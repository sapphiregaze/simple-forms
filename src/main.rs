use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use clap::Parser;
use regex::Regex;
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
    email_regex: Regex,
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

    let allowed_origin = format!("http://{}", args.domain);
    let allowed_origin_https = format!("https://{}", args.domain);

    let governor_conf = GovernorConfigBuilder::default()
        .requests_per_minute(1)
        .burst_size(2)
        .finish()
        .unwrap();

    let regex = Regex::new(
        r"(?i)^([\w-]+(?:\.[\w-]+)*)@((?:[\w-]+\.)*\w[\w-]{0,66})\.([a-z]{2,6}(?:\.[a-z]{2})?)$",
    )
    .unwrap();

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin(&allowed_origin)
            .allowed_origin(&allowed_origin_https)
            .allowed_methods(vec!["GET", "POST", "OPTIONS"])
            .allowed_headers(vec!["Content-Type", "Origin", "Accept"])
            .supports_credentials()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .wrap(Governor::new(&governor_conf))
            .app_data(web::Data::new(AppState {
                db: Mutex::new(Connection::open("contacts.db").expect("Failed to open database")),
                allowed_domain: args.domain.clone(),
                email_regex: regex.clone(),
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

fn validate_form(form: &ContactForm, email_regex: &Regex) -> Result<(), String> {
    if form.name.trim().is_empty() {
        return Err("Name cannot be empty".to_string());
    }

    if form.email.trim().is_empty() {
        return Err("Email cannot be empty".to_string());
    }

    if form.message.trim().is_empty() {
        return Err("Message cannot be empty".to_string());
    }

    if form.name.chars().count() > 50 {
        return Err("Name must be 50 characters or less".to_string());
    }

    if form.email.chars().count() > 50 {
        return Err("Email must be 50 characters or less".to_string());
    }

    if form.subject.chars().count() > 100 {
        return Err("Subject must be 100 characters or less".to_string());
    }

    if form.message.chars().count() > 500 {
        return Err("Message must be 500 characters or less".to_string());
    }

    if !email_regex.is_match(&form.email) {
        return Err("Invalid email format".to_string());
    }

    Ok(())
}

async fn submit_contact(
    req: HttpRequest,
    form: web::Json<ContactForm>,
    data: web::Data<AppState>,
) -> impl Responder {
    let allowed_domain = &data.allowed_domain;

    let origin = match req.headers().get("origin") {
        Some(origin_header) => match origin_header.to_str() {
            Ok(origin_str) => origin_str,
            Err(_) => return HttpResponse::BadRequest().body("Invalid origin header"),
        },
        None => return HttpResponse::BadRequest().body("Missing origin header"),
    };

    let referer = match req.headers().get("referer") {
        Some(referer_header) => match referer_header.to_str() {
            Ok(referer_str) => referer_str,
            Err(_) => return HttpResponse::BadRequest().body("Invalid referer header"),
        },
        None => return HttpResponse::BadRequest().body("Missing referer header"),
    };

    if (!origin.is_empty() && !origin.contains(allowed_domain))
        || (!referer.is_empty() && !referer.contains(allowed_domain))
    {
        return HttpResponse::Forbidden().body("Access denied");
    }

    if let Err(error_message) = validate_form(&form, &data.email_regex) {
        return HttpResponse::BadRequest().json(serde_json::json!({"error": error_message}));
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
