use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use clap::Parser;
use rusqlite::{params, Connection, Result as SqliteResult};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Parser, Debug)]
#[clap(author, version, about = "Contact Form API Server")]
struct Args {
    #[clap(short, long, default_value = "8080")]
    port: u16,

    #[clap(short, long, default_value = "localhost")]
    domain: String,
}

#[derive(Serialize, Deserialize)]
struct ContactForm {
    name: String,
    email: String,
    subject: String,
    message: String,
}

struct AppState {
    db: Mutex<Connection>,
    allowed_domain: String,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let conn = Connection::open("contacts.db").expect("Failed to open database");
    init_db(&conn).expect("Failed to initialize database");

    println!(
        "Starting server on port {} with allowed domain: {}",
        args.port, args.domain
    );

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState {
                db: Mutex::new(Connection::open("contacts.db").expect("Failed to open database")),
                allowed_domain: args.domain.clone(),
            }))
            .route("/contact", web::post().to(submit_contact))
    })
    .bind(format!("0.0.0.0:{}", args.port))?
    .run()
    .await
}

fn init_db(conn: &Connection) -> SqliteResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS contacts (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL,
            subject TEXT NOT NULL,
            message TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    Ok(())
}

async fn submit_contact(
    req: HttpRequest,
    form: web::Json<ContactForm>,
    data: web::Data<AppState>,
) -> impl Responder {
    let allowed_domain = &data.allowed_domain;

    let host = match req.headers().get("host") {
        Some(host_header) => match host_header.to_str() {
            Ok(host_str) => host_str,
            Err(_) => return HttpResponse::BadRequest().body("Invalid host header"),
        },
        None => return HttpResponse::BadRequest().body("Missing host header"),
    };

    if !host.contains(allowed_domain) {
        return HttpResponse::Forbidden().json(
            serde_json::json!({"error": format!("Access denied. Only requests from {} are allowed", allowed_domain)})
        );
    }

    let referer = match req.headers().get("referer") {
        Some(referer_header) => match referer_header.to_str() {
            Ok(referer_str) => referer_str,
            Err(_) => return HttpResponse::BadRequest().body("Invalid referer header"),
        },
        None => "",
    };

    if !referer.is_empty() && !referer.contains(allowed_domain) {
        return HttpResponse::Forbidden().json(
            serde_json::json!({"error": format!("Access denied. Only requests from {} are allowed", allowed_domain)})
        );
    }

    let db = data.db.lock().unwrap();
    let result = db.execute(
        "INSERT INTO contacts (name, email, subject, message) VALUES (?1, ?2, ?3, ?4)",
        params![form.name, form.email, form.subject, form.message],
    );

    match result {
        Ok(_) => HttpResponse::Created()
            .json(serde_json::json!({"message": "Contact form submitted successfully"})),
        Err(e) => {
            eprintln!("Database error: {}", e);
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "Failed to store contact form"}))
        }
    }
}
