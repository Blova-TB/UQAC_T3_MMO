use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};

use crate::jwt::decode_jwt;

pub struct AuthenticatedUser {
    pub user_id: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthenticatedUser {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let auth_header = req.headers().get_one("Authorization");

        let Some(token) = auth_header.and_then(|header| header.strip_prefix("Bearer ")) else {
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
