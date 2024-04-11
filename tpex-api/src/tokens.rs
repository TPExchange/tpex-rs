use std::str::FromStr;

use axum::{async_trait, http::StatusCode};
use axum_extra::headers::{authorization::Bearer, Authorization, HeaderMapExt};
use num_traits::FromPrimitive;
use tpex::PlayerId;
use crate::shared::*;


#[async_trait]
impl axum::extract::FromRequestParts<super::State> for TokenInfo {
    type Rejection = StatusCode;

    #[allow(clippy::type_complexity,clippy::type_repetition_in_bounds)]
    async fn from_request_parts(parts: &mut axum::http::request::Parts, state: &super::State) -> Result<Self, Self::Rejection> {
            let Some(auth) : Option<Authorization<Bearer>> = parts.headers.typed_get()
            else { return Err(StatusCode::UNAUTHORIZED); };

            let Ok(token) = auth.0.token().parse()
            else { return Err(StatusCode::UNAUTHORIZED); };

            let Ok(token_info) = state.tokens.get_token(&token).await
            else { return Err(StatusCode::UNAUTHORIZED); };

            // If the token would need banker perms to make, check that the user is still at that level
            if token_info.level > TokenLevel::ProxyOne && !state.tpex.read().await.state.is_banker(&token_info.user) {
                return Err(StatusCode::UNAUTHORIZED)
            }

            Ok(token_info)
        }
}

pub struct TokenHandler {
    pool: sqlx::SqlitePool
}
impl TokenHandler {
    pub async fn new(url: &str) -> sqlx::Result<TokenHandler> {
        let opt = sqlx::sqlite::SqliteConnectOptions::from_str(url)?.create_if_missing(true);
        let ret = TokenHandler{
            pool: sqlx::SqlitePool::connect_with(opt).await?
        };

        sqlx::migrate!("../migrations/api").run(&ret.pool).await?;

        Ok(ret)
    }
    pub async fn create_token(&self, level: TokenLevel, user: PlayerId) -> sqlx::Result<Token> {
        let token = Token::generate();

        let slice = token.0.as_slice();
        let level = level as i64;
        #[allow(deprecated)]
        let user = user.evil_deref();

        sqlx::query!(r#"INSERT INTO tokens(token, level, user) VALUES (?, ?, ?)"#, slice, level, user)
        .execute(&self.pool).await?;

        Ok(token)
    }
    pub async fn get_token(&self, token: &Token) -> sqlx::Result<TokenInfo> {
        let slice = token.0.as_slice();
        let query =
            sqlx::query!(r#"SELECT token as "token: Vec<u8>", level, user FROM tokens WHERE token = ?"#, slice)
            .fetch_one(&self.pool).await?;

        Ok(TokenInfo {
            token: Token(query.token.try_into().expect("Mismatched token length")),
            #[allow(deprecated)]
            user: tpex::PlayerId::evil_constructor(query.user),
            level: TokenLevel::from_i64(query.level).expect("Invalid token level")
        })
    }
    pub async fn delete_token(&self, token: &Token) -> sqlx::Result<()> {
        let slice = token.0.as_slice();
        sqlx::query!(r#"DELETE FROM tokens WHERE token = ?"#, slice)
        .execute(&self.pool).await?;
        Ok(())
    }
}
