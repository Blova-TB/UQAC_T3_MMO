use crate::web_models::{AuthenticatedUser, BasicCredentials, Claims};
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, encode};

#[rocket::async_trait]
impl<'r> FromRequest<'r> for BasicCredentials {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let Some(header) = req.headers().get_one("Authorization") else {
            return Outcome::Forward(Status::Unauthorized);
        };

        match parse_basic_credentials(header) {
            Some(credentials) => Outcome::Success(credentials),
            None => Outcome::Forward(Status::Unauthorized),
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthenticatedUser {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let auth_header = req.headers().get_one("Authorization");

        let Some(token) = auth_header.and_then(parse_bearer_token) else {
            return Outcome::Forward(Status::Unauthorized);
        };

        let Some(secret) = req.rocket().state::<String>() else {
            return Outcome::Error((Status::InternalServerError, ()));
        };

        match decode_jwt(token, secret) {
            Ok(claims) => Outcome::Success(AuthenticatedUser {
                user_id: claims.sub,
                custom_id: claims.custom_id,
                pos_x: claims.pos_x,
                pos_y: claims.pos_y,
            }),
            Err(_) => Outcome::Forward(Status::Unauthorized),
        }
    }
}

fn parse_basic_credentials(header: &str) -> Option<BasicCredentials> {
    let encoded = header.strip_prefix("Basic ")?;
    let decoded = STANDARD.decode(encoded).ok()?;
    let credentials = String::from_utf8(decoded).ok()?;
    let (username, password) = credentials.split_once(':')?;

    Some(BasicCredentials {
        username: username.to_owned(),
        password: password.to_owned(),
    })
}

fn parse_bearer_token(header: &str) -> Option<&str> {
    header.strip_prefix("Bearer ")
}

pub fn create_jwt(
    user_id: &str,
    custom_id: u32,
    pos_x: f32,
    pos_y: f32,
    secret: &str,
    duration_hours: i64
) -> Result<String, jsonwebtoken::errors::Error> {
    let expiration = Utc::now()
        .checked_add_signed(Duration::hours(duration_hours))
        .expect("valid timestamp")
        .timestamp();

    let claims = Claims {
        sub: user_id.to_owned(),
        custom_id,
        pos_x,
        pos_y,
        exp: expiration as usize,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
}

pub fn decode_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::default(),
    )
        .map(|data| data.claims)
}
