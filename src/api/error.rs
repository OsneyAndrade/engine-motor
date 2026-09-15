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
            hint: hint_for(code),
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
        ApiError::new(e.status(), e.code(), e.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codigos_com_dica_a_trazem_em_qualquer_construtor() {
        assert!(ApiError::forbidden("x").hint.is_some());
        assert!(ApiError::bad_request("unsupported_codec", "x").hint.is_some());
        assert!(ApiError::new(StatusCode::CONFLICT, "dictionary_unavailable", "x").hint.is_some());
        let convertido: ApiError = EngineError::not_found("x").into();
        assert!(convertido.hint.is_none());
    }

    #[test]
    fn with_hint_sobrepoe_a_dica_padrao() {
        let e = ApiError::forbidden("x").with_hint("dica especifica");
        assert_eq!(e.hint.as_deref(), Some("dica especifica"));
    }

    #[test]
    fn codigo_sem_dica_fica_sem_dica() {
        assert!(ApiError::bad_request("empty_body", "x").hint.is_none());
        assert!(ApiError::internal("x").hint.is_none());
    }
}
