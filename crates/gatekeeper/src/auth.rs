use base64::{engine::general_purpose::STANDARD, Engine as _};
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};

use crate::jwt::decode_jwt;
use crate::models::{AuthenticatedUser, BasicCredentials};

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
            }),
            Err(_) => Outcome::Forward(Status::Unauthorized),
        }
    }
}

