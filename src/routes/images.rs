use actix::clock::Duration;
use actix_web::{body::BodyStream, http::StatusCode, *};
use awc::Client;
use lemmy_rate_limit::RateLimit;
use lemmy_utils::settings::Settings;
use serde::{Deserialize, Serialize};

pub fn config(cfg: &mut web::ServiceConfig, rate_limit: &RateLimit) {
  let client = Client::builder()
    .header("User-Agent", "pict-rs-frontend, v0.1.0")
    .timeout(Duration::from_secs(30))
    .finish();

  cfg
    .data(client)
    .service(
      web::resource("/pictrs/image")
        .wrap(rate_limit.image())
        .route(web::post().to(upload)),
    )
    // This has optional query params: /image/{filename}?format=jpg&thumbnail=256
    .service(web::resource("/pictrs/image/{filename}").route(web::get().to(full_res)))
    .service(web::resource("/pictrs/image/delete/{token}/{filename}").route(web::get().to(delete)));
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Image {
  file: String,
  delete_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Images {
  msg: String,
  files: Option<Vec<Image>>,
}

#[derive(Deserialize)]
pub struct PictrsParams {
  format: Option<String>,
  thumbnail: Option<String>,
}

async fn upload(
  req: HttpRequest,
  body: web::Payload,
  client: web::Data<Client>,
) -> Result<HttpResponse, Error> {
  // TODO: check auth and rate limit here

  let mut res = client
    .request_from(format!("{}/image", Settings::get().pictrs_url), req.head())
    .if_some(req.head().peer_addr, |addr, req| {
      req.header("X-Forwarded-For", addr.to_string())
    })
    .send_stream(body)
    .await?;

  let images = res.json::<Images>().await?;

  Ok(HttpResponse::build(res.status()).json(images))
}

async fn full_res(
  filename: web::Path<String>,
  web::Query(params): web::Query<PictrsParams>,
  req: HttpRequest,
  client: web::Data<Client>,
) -> Result<HttpResponse, Error> {
  let name = &filename.into_inner();

  // If there are no query params, the URL is original
  let url = if params.format.is_none() && params.thumbnail.is_none() {
    format!("{}/image/original/{}", Settings::get().pictrs_url, name,)
  } else {
    // Use jpg as a default when none is given
    let format = params.format.unwrap_or_else(|| "jpg".to_string());

    let mut url = format!(
      "{}/image/process.{}?src={}",
      Settings::get().pictrs_url,
      format,
      name,
    );

    if let Some(size) = params.thumbnail {
      url = format!("{}&thumbnail={}", url, size,);
    }
    url
  };

  image(url, req, client).await
}

async fn image(
  url: String,
  req: HttpRequest,
  client: web::Data<Client>,
) -> Result<HttpResponse, Error> {
  let res = client
    .request_from(url, req.head())
    .if_some(req.head().peer_addr, |addr, req| {
      req.header("X-Forwarded-For", addr.to_string())
    })
    .no_decompress()
    .send()
    .await?;

  if res.status() == StatusCode::NOT_FOUND {
    return Ok(HttpResponse::NotFound().finish());
  }

  let mut client_res = HttpResponse::build(res.status());

  for (name, value) in res.headers().iter().filter(|(h, _)| *h != "connection") {
    client_res.header(name.clone(), value.clone());
  }

  Ok(client_res.body(BodyStream::new(res)))
}

async fn delete(
  components: web::Path<(String, String)>,
  req: HttpRequest,
  client: web::Data<Client>,
) -> Result<HttpResponse, Error> {
  let (token, file) = components.into_inner();

  let url = format!(
    "{}/image/delete/{}/{}",
    Settings::get().pictrs_url,
    &token,
    &file
  );
  let res = client
    .request_from(url, req.head())
    .if_some(req.head().peer_addr, |addr, req| {
      req.header("X-Forwarded-For", addr.to_string())
    })
    .no_decompress()
    .send()
    .await?;

  Ok(HttpResponse::build(res.status()).body(BodyStream::new(res)))
}
