use actix_web::{get, HttpResponse};

#[get("/healthz")]
pub async fn healthz() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "status": "ok" }))
}
