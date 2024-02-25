use actix_cors::Cors;
use actix_web::{http, middleware, Result};
use lambda_runtime::Error;
use lambda_web::actix_web::{self, App, HttpServer};
use lambda_web::{is_running_on_lambda, run_actix_on_lambda};
use log_service_writer::api;

#[actix_web::main]
async fn main() -> Result<(), Error> {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();

    let service_port = if args.len() > 1 {
        args[1].parse::<u16>().expect("Invalid port argument")
    } else {
        8000
    };

    let factory = move || {
        let cors = Cors::default()
            .allow_any_origin() // TODO: Tighten down prod origins
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![http::header::AUTHORIZATION, http::header::ACCEPT])
            .allowed_header(http::header::CONTENT_TYPE)
            .supports_credentials()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .wrap(middleware::Compress::default())
            .service(api::get_logs_endpoint)
            .service(api::create_logs_endpoint)
    };

    if is_running_on_lambda() {
        run_actix_on_lambda(factory).await?;
    } else {
        HttpServer::new(factory)
            .bind(format!("0.0.0.0:{service_port}"))?
            .run()
            .await?;
    }
    Ok(())
}
