use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::service::EngineError;

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
    pub request_id: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            code,
            message: message.into(),
            hint: None,
            request_id: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }
}

impl From<EngineError> for ApiError {
    fn from(e: EngineError) -> Self {
        let status = e.status();
        let code = e.code();
        let hint = hint_for(code);
        ApiError {
            status,
            code,
            message: e.to_string(),
            hint,
            request_id: None,
        }
    }
}

fn hint_for(code: &str) -> Option<String> {
    let h = match code {
        "envelope_invalid" => {
            "O corpo deve ser um container .syntra (protobuf) produzido por POST /api/v1/compress."
        }
        "envelope_corrupted" => {
            "O checksum do payload não confere. Rebaixe para a última cópia boa ou recomprima o original."
        }
        "dictionary_unavailable" => {
            "Esta essência depende de um dicionário treinado. Restaure o diretório SYNTRA_DICT_PATH ou aponte SYNTRA_REDIS_URL para o cluster que o contém."
        }
        "dictionary_mismatch" => {
            "O dicionário com este id tem conteúdo diferente do usado na compressão. Verifique se o diretório de dicionários foi sobrescrito."
        }
        "fidelity_mismatch" => {
            "A reconstrução não bate com o hash do original — indício de corrupção de dados. Não use este resultado."
        }
        "unsupported_codec" => "Consulte GET /api/v1/codecs para a lista de codecs e faixas de nível.",
        "forbidden" => "A chave usada não tem o escopo necessário para esta rota. Emita outra em POST /api/v1/admin/tenants/{id}/keys.",
        "overloaded" => "Todos os permits de CPU estão em uso. Tente novamente com backoff.",
        _ => return None,
    };
    Some(h.to_string())
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            tracing::error!(code = self.code, mensagem = self.message, "erro na API");
        } else {
            tracing::debug!(code = self.code, mensagem = self.message, "requisição rejeitada");
        }

        (
            self.status,
            Json(ErrorResponse {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                    request_id: self.request_id,
                    hint: self.hint,
                },
            }),
        )
            .into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
