use axum::Json;

use crate::codec::CATALOG;

pub async fn document() -> Json<serde_json::Value> {
    let codec_names: Vec<&str> = CATALOG.iter().map(|c| c.name).collect();

    Json(serde_json::json!({
      "openapi": "3.0.3",
      "info": {
        "title": "Syntra Engine API",
        "version": env!("CARGO_PKG_VERSION"),
        "description":
          "Compressão sem perdas com seleção de plano por medição.\n\n\
           O motor gera um container `.syntra` auto-suficiente: além dos bytes \
           comprimidos, o envelope guarda a receita completa de reconstrução \
           (pilha de transforms, codec, nível, id do dicionário) e o BLAKE3 do \
           original. Reconstruir exige apenas o container.\n\n\
           **Garantias**\n\
           - Reconstrução bit-a-bit, conferida por hash em toda descompressão.\n\
           - A essência nunca fica maior que o original (piso `stored`).\n\
           - Dicionários treinados são endereçados por conteúdo: retreinar \
             jamais invalida essências já gravadas.",
        "contact": { "name": "Syntra Engine" }
      },
      "servers": [
        { "url": "/", "description": "Instância atual" }
      ],
      "tags": [
        { "name": "compressão", "description": "Gerar e desfazer essências" },
        { "name": "objetos", "description": "Essências arquivadas no vault" },
        { "name": "descoberta", "description": "Capacidades do motor" },
        { "name": "operação", "description": "Saúde e métricas" },
        { "name": "consumo", "description": "Medição de uso e cobrança" },
        { "name": "administração", "description": "Tenants e chaves de API" }
      ],
      "security": [{ "ApiKeyHeader": [] }, { "BearerToken": [] }],
      "components": {
        "securitySchemes": {
          "ApiKeyHeader": {
            "type": "apiKey", "in": "header", "name": "x-api-key",
            "description": "Chave no formato syn_<id>_<segredo>, emitida em POST /api/v1/admin/tenants/{id}/keys. A chave determina o tenant e os escopos; o servidor guarda apenas o hash. Chaves estáticas de SYNTRA_API_KEYS também são aceitas e operam no tenant 'default' com todos os escopos."
          },
          "BearerToken": {
            "type": "http", "scheme": "bearer",
            "description": "Alternativa: Authorization: Bearer <chave>."
          }
        },
        "parameters": {
          "filename": {
            "name": "filename", "in": "query", "required": false,
            "schema": { "type": "string" },
            "description": "Nome do original. Também aceito no header x-filename. Usado para detectar MIME e para auditoria — nunca compõe caminho em disco."
          },
          "mime": {
            "name": "mime", "in": "query", "required": false,
            "schema": { "type": "string" },
            "description": "MIME declarado; sobrepõe a detecção automática. Header: x-mime."
          },
          "effort": {
            "name": "effort", "in": "query", "required": false,
            "schema": { "type": "string", "enum": ["fast", "balanced", "max"] },
            "description": "Quantos planos candidatos medir. fast=1, balanced=4, max=16. Header: x-effort."
          },
          "codec": {
            "name": "codec", "in": "query", "required": false,
            "schema": { "type": "string", "enum": codec_names },
            "description": "Força um codec e desativa a medição de candidatos. Header: x-codec."
          },
          "level": {
            "name": "level", "in": "query", "required": false,
            "schema": { "type": "integer" },
            "description": "Nível do codec forçado. Fora da faixa devolve 400 invalid_level. Header: x-level."
          },
          "verify": {
            "name": "verify", "in": "query", "required": false,
            "schema": { "type": "boolean" },
            "description": "Confere o round-trip antes de gravar. Custa uma descompressão. Header: x-verify."
          },
          "persist": {
            "name": "persist", "in": "query", "required": false,
            "schema": { "type": "boolean", "default": true },
            "description": "false devolve a essência sem gravar no vault."
          },
          "dictionary": {
            "name": "dictionary", "in": "query", "required": false,
            "schema": { "type": "boolean" },
            "description": "Permite usar dicionário treinado da classe de conteúdo. Header: x-dictionary."
          },
          "dedup": {
            "name": "dedup", "in": "query", "required": false,
            "schema": { "type": "boolean", "default": true },
            "description": "Reaproveita a essência existente quando o conteúdo é idêntico."
          },
          "response": {
            "name": "response", "in": "query", "required": false,
            "schema": { "type": "string", "enum": ["binary", "json"] },
            "description": "binary devolve o container; json devolve o relatório. Accept: application/json equivale a json."
          },
          "idempotencyKey": {
            "name": "Idempotency-Key", "in": "header", "required": false,
            "schema": { "type": "string", "maxLength": 200 },
            "description": "Repetir a mesma chave para o mesmo tenant devolve o resultado anterior sem reprocessar e sem gerar novo evento de cobrança."
          },
          "tenantId": {
            "name": "id", "in": "path", "required": true,
            "schema": { "type": "string", "pattern": "^[A-Za-z0-9._-]{1,64}$" },
            "description": "Identificador do tenant."
          },
          "usageFrom": {
            "name": "from_ms", "in": "query", "required": false,
            "schema": { "type": "integer", "format": "int64" },
            "description": "Início da janela em epoch millis. Default: to_ms menos `days`."
          },
          "usageTo": {
            "name": "to_ms", "in": "query", "required": false,
            "schema": { "type": "integer", "format": "int64" },
            "description": "Fim da janela em epoch millis. Default: agora."
          },
          "usageDays": {
            "name": "days", "in": "query", "required": false,
            "schema": { "type": "integer", "default": 30, "maximum": 366 },
            "description": "Janela em dias quando from_ms não é informado."
          },
          "usageEvents": {
            "name": "events", "in": "query", "required": false,
            "schema": { "type": "integer" },
            "description": "Quantos eventos recentes incluir na resposta."
          },
          "objectId": {
            "name": "id", "in": "path", "required": true,
            "schema": { "type": "string", "pattern": "^[0-9a-fA-F]{64}$" },
            "description": "Hash BLAKE3 do original, 64 dígitos hexadecimais."
          }
        },
        "schemas": {
          "Error": {
            "type": "object",
            "required": ["error"],
            "properties": {
              "error": {
                "type": "object",
                "required": ["code", "message"],
                "properties": {
                  "code": {
                    "type": "string",
                    "description": "Código estável para tratamento programático.",
                    "enum": [
                      "invalid_object_id", "invalid_effort", "invalid_level",
                      "unsupported_codec", "empty_body", "not_found",
                      "unauthorized", "forbidden", "invalid_tenant_id",
                      "invalid_scope", "invalid_plan", "invalid_name",
                      "invalid_expiry", "missing_tenant",
                      "envelope_invalid", "envelope_malformed",
                      "envelope_corrupted", "dictionary_unavailable",
                      "dictionary_mismatch", "restore_failed",
                      "fidelity_mismatch", "io_error", "internal_error",
                      "overloaded"
                    ]
                  },
                  "message": { "type": "string" },
                  "hint": { "type": "string", "description": "Ação sugerida, quando aplicável." },
                  "request_id": { "type": "string" }
                }
              }
            }
          },
          "Plan": {
            "type": "object",
            "description": "Receita de reconstrução efetivamente usada.",
            "properties": {
              "label": { "type": "string", "example": "csv_columnar+zstd:19" },
              "codec": { "type": "string", "enum": codec_names },
              "level": { "type": "integer" },
              "transforms": { "type": "string", "example": "csv_columnar>delta:1" },
              "dictionary_id": { "type": "string", "nullable": true }
            }
          },
          "CompressResponse": {
            "type": "object",
            "properties": {
              "id": { "type": "string", "description": "Hash BLAKE3 do original." },
              "original_name": { "type": "string" },
              "mime": { "type": "string" },
              "content_class": {
                "type": "string",
                "enum": ["tabular", "structured_text", "plain_text", "numeric_binary",
                         "binary", "pre_compressed", "media", "incompressible"]
              },
              "original_size": { "type": "integer", "format": "int64" },
              "essence_size": { "type": "integer", "format": "int64" },
              "savings_pct": { "type": "number" },
              "ratio": { "type": "number", "description": "essence_size / original_size." },
              "plan": { "$ref": "#/components/schemas/Plan" },
              "reason": { "type": "string", "description": "Por que este plano venceu." },
              "candidates_tried": { "type": "integer" },
              "verified": { "type": "boolean", "description": "Round-trip conferido antes de gravar." },
              "deduplicated": { "type": "boolean", "description": "O próprio tenant já tinha este conteúdo. Deduplicação entre tenants não é reportada." },
              "sampled_decision": { "type": "boolean", "description": "Plano escolhido medindo amostra, não o arquivo inteiro." },
              "replayed": { "type": "boolean", "description": "Resposta de uma requisição idempotente anterior." },
              "persisted": { "type": "boolean" },
              "references": { "type": "integer", "nullable": true },
              "timings": {
                "type": "object",
                "properties": {
                  "compress_ms": { "type": "number" },
                  "verify_ms": { "type": "number" },
                  "total_ms": { "type": "number" }
                }
              },
              "links": {
                "type": "object",
                "properties": {
                  "metadata": { "type": "string" },
                  "content": { "type": "string" },
                  "essence": { "type": "string" }
                }
              }
            }
          },
          "AnalyzeResponse": {
            "type": "object",
            "description": "Comparativo medido de todos os planos candidatos.",
            "properties": {
              "original_name": { "type": "string" },
              "mime": { "type": "string" },
              "content_class": { "type": "string" },
              "original_size": { "type": "integer", "format": "int64" },
              "measured_bytes": { "type": "integer", "description": "Bytes efetivamente medidos (amostra em arquivos grandes)." },
              "entropy_bits_per_byte": { "type": "number" },
              "detected_stride": { "type": "integer", "nullable": true },
              "detected_columns": { "type": "integer", "nullable": true },
              "effort": { "type": "string" },
              "dictionary_id": { "type": "string", "nullable": true },
              "recommended": { "type": "string", "nullable": true },
              "candidates": {
                "type": "array",
                "items": {
                  "type": "object",
                  "properties": {
                    "plan": { "type": "string" },
                    "codec": { "type": "string" },
                    "level": { "type": "integer" },
                    "transforms": { "type": "string" },
                    "used_dictionary": { "type": "boolean" },
                    "output_size": { "type": "integer" },
                    "savings_pct": { "type": "number" },
                    "encode_ms": { "type": "number" },
                    "error": { "type": "string", "nullable": true }
                  }
                }
              }
            }
          },
          "ObjectResponse": {
            "type": "object",
            "properties": {
              "id": { "type": "string" },
              "original_name": { "type": "string" },
              "known_names": { "type": "array", "items": { "type": "string" } },
              "mime": { "type": "string" },
              "content_class": { "type": "string" },
              "original_size": { "type": "integer", "format": "int64" },
              "essence_size": { "type": "integer", "format": "int64" },
              "savings_pct": { "type": "number" },
              "plan": { "$ref": "#/components/schemas/Plan" },
              "reason": { "type": "string" },
              "verified": { "type": "boolean" },
              "references": { "type": "integer" },
              "container_version": { "type": "integer" },
              "created_at_ms": { "type": "integer", "format": "int64" },
              "updated_at_ms": { "type": "integer", "format": "int64" }
            }
          },
          "Quote": {
            "type": "object",
            "description": "Valor calculado para a janela, segundo o plano do tenant.",
            "properties": {
              "plan": { "type": "string", "enum": ["savings_share", "ingested_gb"] },
              "formula": { "type": "string", "description": "A fórmula aplicada, em texto." },
              "currency": { "type": "string" },
              "amount_micro_cents": { "type": "integer", "format": "int64", "description": "Valor exato em 10^-6 centavos. Use este campo para acumular e fechar competência: arredondar para centavo por linha zera volumes pequenos." },
              "amount_cents": { "type": "integer", "format": "int64", "description": "Total arredondado, para exibição." },
              "lines": {
                "type": "array",
                "items": {
                  "type": "object",
                  "properties": {
                    "label": { "type": "string" },
                    "quantity_gib": { "type": "number" },
                    "unit_cents_per_gib": { "type": "number" },
                    "amount_micro_cents": { "type": "integer", "format": "int64" }
                  }
                }
              }
            }
          },
          "UsageResponse": {
            "type": "object",
            "properties": {
              "tenant_id": { "type": "string" },
              "from_ms": { "type": "integer", "format": "int64" },
              "to_ms": { "type": "integer", "format": "int64" },
              "totals": {
                "type": "object",
                "properties": {
                  "events": { "type": "integer" },
                  "bytes_in": { "type": "integer", "format": "int64" },
                  "bytes_out": { "type": "integer", "format": "int64" },
                  "bytes_saved": { "type": "integer", "format": "int64" },
                  "savings_pct": { "type": "number" },
                  "cpu_ms": { "type": "number" },
                  "dedup_same_tenant": { "type": "integer" },
                  "dedup_cross_tenant": { "type": "integer" },
                  "by_operation": { "type": "array", "items": { "type": "object" } },
                  "by_effort": { "type": "array", "items": { "type": "object" } }
                }
              },
              "quote": { "$ref": "#/components/schemas/Quote" },
              "recent": { "type": "array", "items": { "type": "object" } }
            }
          },
          "PriceConfig": {
            "type": "object",
            "description": "Parâmetros das duas fórmulas. O plano do tenant decide qual é aplicada.",
            "properties": {
              "share_pct": { "type": "number", "default": 30, "description": "Percentual da economia cobrado (savings_share)." },
              "reference_gb_month_cents": { "type": "number", "default": 12, "description": "Custo de referência de GiB-mês do storage que o cliente evita." },
              "per_gb_cents": {
                "type": "object",
                "description": "Centavos por GiB ingerido, por tier de esforço (ingested_gb).",
                "properties": {
                  "fast": { "type": "number", "default": 150 },
                  "balanced": { "type": "number", "default": 400 },
                  "max": { "type": "number", "default": 1600 }
                }
              },
              "currency": { "type": "string", "enum": ["BRL", "USD"], "default": "BRL" }
            }
          },
          "Tenant": {
            "type": "object",
            "properties": {
              "id": { "type": "string" },
              "name": { "type": "string" },
              "plan": { "type": "string", "enum": ["savings_share", "ingested_gb"] },
              "price": { "$ref": "#/components/schemas/PriceConfig" },
              "active": { "type": "boolean" },
              "created_at_ms": { "type": "integer", "format": "int64" }
            }
          },
          "ApiKey": {
            "type": "object",
            "properties": {
              "key_id": { "type": "string" },
              "tenant_id": { "type": "string" },
              "name": { "type": "string" },
              "scopes": { "type": "array", "items": { "type": "string", "enum": ["compress", "read", "delete", "admin"] } },
              "expires_at_ms": { "type": "integer", "format": "int64", "nullable": true },
              "revoked_at_ms": { "type": "integer", "format": "int64", "nullable": true },
              "last_used_at_ms": { "type": "integer", "format": "int64", "nullable": true },
              "created_at_ms": { "type": "integer", "format": "int64" }
            }
          },
          "IssuedKey": {
            "allOf": [
              { "$ref": "#/components/schemas/ApiKey" },
              {
                "type": "object",
                "properties": {
                  "key": { "type": "string", "description": "O segredo completo. Aparece SÓ nesta resposta: o servidor guarda apenas o hash." },
                  "warning": { "type": "string" }
                }
              }
            ]
          },
          "DeleteResponse": {
            "type": "object",
            "properties": {
              "id": { "type": "string" },
              "removed": { "type": "boolean", "description": "true se a essência saiu do disco." },
              "remaining_references": { "type": "integer", "nullable": true }
            }
          }
        },
        "responses": {
          "BadRequest": {
            "description": "Parâmetro inválido.",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
          },
          "Unauthorized": {
            "description": "Credencial ausente ou inválida.",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
          },
          "NotFound": {
            "description": "Objeto não encontrado.",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
          },
          "Unprocessable": {
            "description": "Essência corrompida, infiel ao original ou irreconstruível.",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
          },
          "Forbidden": {
            "description": "A chave é válida mas não tem o escopo exigido pela rota.",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
          }
        }
      },
      "paths": {
        "/api/v1/compress": {
          "post": {
            "tags": ["compressão"],
            "summary": "Comprime um conteúdo",
            "description":
              "O corpo é o conteúdo cru. Por padrão o motor mede vários planos \
               candidatos e grava o menor resultado; forçar `codec` desliga a \
               medição. A essência nunca fica maior que o original.",
            "parameters": [
              { "$ref": "#/components/parameters/filename" },
              { "$ref": "#/components/parameters/mime" },
              { "$ref": "#/components/parameters/effort" },
              { "$ref": "#/components/parameters/codec" },
              { "$ref": "#/components/parameters/level" },
              { "$ref": "#/components/parameters/verify" },
              { "$ref": "#/components/parameters/persist" },
              { "$ref": "#/components/parameters/dictionary" },
              { "$ref": "#/components/parameters/dedup" },
              { "$ref": "#/components/parameters/response" },
              { "$ref": "#/components/parameters/idempotencyKey" }
            ],
            "requestBody": {
              "required": true,
              "content": { "application/octet-stream": { "schema": { "type": "string", "format": "binary" } } }
            },
            "responses": {
              "200": {
                "description": "Essência gerada.",
                "headers": {
                  "x-syntra-id": { "schema": { "type": "string" }, "description": "Hash BLAKE3 do original." },
                  "x-syntra-original-size": { "schema": { "type": "integer" } },
                  "x-syntra-essence-size": { "schema": { "type": "integer" } },
                  "x-syntra-savings-pct": { "schema": { "type": "number" } },
                  "x-syntra-plan": { "schema": { "type": "string" } },
                  "x-syntra-content-class": { "schema": { "type": "string" } },
                  "x-syntra-verified": { "schema": { "type": "boolean" } },
                  "x-syntra-deduplicated": { "schema": { "type": "boolean" } }
                },
                "content": {
                  "application/x-syntra-essence": { "schema": { "type": "string", "format": "binary" } },
                  "application/json": { "schema": { "$ref": "#/components/schemas/CompressResponse" } }
                }
              },
              "400": { "$ref": "#/components/responses/BadRequest" },
              "401": { "$ref": "#/components/responses/Unauthorized" },
              "403": { "$ref": "#/components/responses/Forbidden" },
              "422": { "$ref": "#/components/responses/Unprocessable" },
              "503": { "description": "Motor encerrando." }
            }
          }
        },
        "/api/v1/decompress": {
          "post": {
            "tags": ["compressão"],
            "summary": "Reconstrói o original a partir de uma essência",
            "description":
              "O corpo é um container `.syntra`. O BLAKE3 do resultado é \
               conferido contra o gravado no envelope: divergência devolve 422 \
               em vez de dado adulterado.",
            "parameters": [{ "$ref": "#/components/parameters/response" }],
            "requestBody": {
              "required": true,
              "content": { "application/x-syntra-essence": { "schema": { "type": "string", "format": "binary" } } }
            },
            "responses": {
              "200": {
                "description": "Conteúdo original.",
                "headers": {
                  "x-syntra-filename": { "schema": { "type": "string" } },
                  "x-syntra-fidelity-checked": { "schema": { "type": "boolean" } },
                  "x-syntra-reconstruct-ms": { "schema": { "type": "number" } }
                },
                "content": { "application/octet-stream": { "schema": { "type": "string", "format": "binary" } } }
              },
              "400": { "$ref": "#/components/responses/BadRequest" },
              "409": { "description": "Dicionário necessário indisponível neste nó." },
              "422": { "$ref": "#/components/responses/Unprocessable" }
            }
          }
        },
        "/api/v1/analyze": {
          "post": {
            "tags": ["compressão"],
            "summary": "Mede todos os planos candidatos sem gravar nada",
            "description":
              "Devolve o trade-off real (tamanho × tempo) de cada plano, para \
               escolher o modo com número medido em vez de heurística.",
            "parameters": [
              { "$ref": "#/components/parameters/filename" },
              { "$ref": "#/components/parameters/mime" },
              { "$ref": "#/components/parameters/effort" }
            ],
            "requestBody": {
              "required": true,
              "content": { "application/octet-stream": { "schema": { "type": "string", "format": "binary" } } }
            },
            "responses": {
              "200": {
                "description": "Comparativo ordenado do menor para o maior resultado.",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AnalyzeResponse" } } }
              },
              "400": { "$ref": "#/components/responses/BadRequest" }
            }
          }
        },
        "/api/v1/inspect": {
          "post": {
            "tags": ["compressão"],
            "summary": "Lê os metadados de uma essência",
            "description": "Decodifica o container, valida o checksum do payload e devolve o plano gravado. Não reconstrói o conteúdo.",
            "requestBody": {
              "required": true,
              "content": { "application/x-syntra-essence": { "schema": { "type": "string", "format": "binary" } } }
            },
            "responses": {
              "200": { "description": "Metadados do container.", "content": { "application/json": { "schema": { "type": "object" } } } },
              "400": { "$ref": "#/components/responses/BadRequest" },
              "422": { "$ref": "#/components/responses/Unprocessable" }
            }
          }
        },
        "/api/v1/objects": {
          "get": {
            "tags": ["objetos"],
            "summary": "Lista os objetos do tenant",
            "description": "Só retorna objetos aos quais o tenant tem grant.",
            "parameters": [
              { "name": "search", "in": "query", "schema": { "type": "string" }, "description": "Filtra por nome do original." },
              { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 100, "maximum": 1000 } }
            ],
            "responses": { "200": { "description": "Objetos do tenant." } }
          }
        },
        "/api/v1/objects/{id}": {
          "get": {
            "tags": ["objetos"],
            "summary": "Metadados do objeto",
            "parameters": [{ "$ref": "#/components/parameters/objectId" }],
            "responses": {
              "200": { "description": "Metadados.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ObjectResponse" } } } },
              "400": { "$ref": "#/components/responses/BadRequest" },
              "404": { "$ref": "#/components/responses/NotFound" }
            }
          },
          "delete": {
            "tags": ["objetos"],
            "summary": "Solta uma referência ao objeto",
            "description": "Solta a referência do tenant. A essência só sai do disco quando nenhum tenant mais a referencia. `purge=true` descarta todas as referências deste tenant de uma vez.",
            "parameters": [
              { "$ref": "#/components/parameters/objectId" },
              { "name": "purge", "in": "query", "schema": { "type": "boolean", "default": false } }
            ],
            "responses": {
              "200": { "description": "Resultado da remoção.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/DeleteResponse" } } } },
              "404": { "$ref": "#/components/responses/NotFound" }
            }
          }
        },
        "/api/v1/objects/{id}/content": {
          "get": {
            "tags": ["objetos"],
            "summary": "Baixa o original reconstruído",
            "parameters": [{ "$ref": "#/components/parameters/objectId" }],
            "responses": {
              "200": { "description": "Conteúdo original.", "content": { "application/octet-stream": { "schema": { "type": "string", "format": "binary" } } } },
              "404": { "$ref": "#/components/responses/NotFound" },
              "422": { "$ref": "#/components/responses/Unprocessable" }
            }
          }
        },
        "/api/v1/objects/{id}/essence": {
          "get": {
            "tags": ["objetos"],
            "summary": "Baixa o container .syntra cru",
            "description": "Para replicar a essência em outro storage sem descomprimir.",
            "parameters": [{ "$ref": "#/components/parameters/objectId" }],
            "responses": {
              "200": { "description": "Container.", "content": { "application/x-syntra-essence": { "schema": { "type": "string", "format": "binary" } } } },
              "404": { "$ref": "#/components/responses/NotFound" }
            }
          }
        },
        "/api/v1/usage": {
          "get": {
            "tags": ["consumo"],
            "summary": "Consumo e valor calculado do próprio tenant",
            "description": "Agrega o ledger append-only na janela pedida e aplica a fórmula do plano do tenant. Todas as dimensões ficam registradas (bytes_in, bytes_out, bytes_saved, CPU, deduplicação), então trocar de plano não exige reprocessar histórico.",
            "parameters": [
              { "$ref": "#/components/parameters/usageFrom" },
              { "$ref": "#/components/parameters/usageTo" },
              { "$ref": "#/components/parameters/usageDays" },
              { "$ref": "#/components/parameters/usageEvents" }
            ],
            "responses": {
              "200": { "description": "Totais e cotação.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsageResponse" } } } },
              "401": { "$ref": "#/components/responses/Unauthorized" },
              "403": { "$ref": "#/components/responses/Forbidden" }
            }
          }
        },
        "/api/v1/admin/tenants": {
          "get": {
            "tags": ["administração"],
            "summary": "Lista tenants",
            "responses": {
              "200": { "description": "Tenants.", "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Tenant" } } } } },
              "403": { "$ref": "#/components/responses/Forbidden" }
            }
          },
          "post": {
            "tags": ["administração"],
            "summary": "Cria ou atualiza um tenant",
            "requestBody": {
              "required": true,
              "content": { "application/json": { "schema": {
                "type": "object",
                "required": ["name"],
                "properties": {
                  "id": { "type": "string", "description": "Omitido gera um UUID." },
                  "name": { "type": "string" },
                  "plan": { "type": "string", "enum": ["savings_share", "ingested_gb"] },
                  "price": { "$ref": "#/components/schemas/PriceConfig" },
                  "active": { "type": "boolean" }
                }
              } } }
            },
            "responses": {
              "200": { "description": "Tenant.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Tenant" } } } },
              "400": { "$ref": "#/components/responses/BadRequest" },
              "403": { "$ref": "#/components/responses/Forbidden" }
            }
          }
        },
        "/api/v1/admin/tenants/{id}": {
          "get": {
            "tags": ["administração"],
            "summary": "Detalhe do tenant",
            "parameters": [{ "$ref": "#/components/parameters/tenantId" }],
            "responses": {
              "200": { "description": "Tenant.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Tenant" } } } },
              "404": { "$ref": "#/components/responses/NotFound" }
            }
          }
        },
        "/api/v1/admin/tenants/{id}/keys": {
          "get": {
            "tags": ["administração"],
            "summary": "Lista chaves do tenant",
            "description": "Nunca devolve o segredo — apenas metadados.",
            "parameters": [{ "$ref": "#/components/parameters/tenantId" }],
            "responses": {
              "200": { "description": "Chaves.", "content": { "application/json": { "schema": { "type": "array", "items": { "$ref": "#/components/schemas/ApiKey" } } } } },
              "404": { "$ref": "#/components/responses/NotFound" }
            }
          },
          "post": {
            "tags": ["administração"],
            "summary": "Emite uma chave de API",
            "description": "O segredo aparece SÓ nesta resposta. O servidor guarda apenas o BLAKE3 e não há como recuperá-lo.",
            "parameters": [{ "$ref": "#/components/parameters/tenantId" }],
            "requestBody": {
              "required": false,
              "content": { "application/json": { "schema": {
                "type": "object",
                "properties": {
                  "name": { "type": "string" },
                  "scopes": { "type": "array", "items": { "type": "string", "enum": ["compress", "read", "delete", "admin"] }, "default": ["compress", "read"] },
                  "expires_in_days": { "type": "integer", "minimum": 1, "maximum": 3650 }
                }
              } } }
            },
            "responses": {
              "200": { "description": "Chave emitida.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/IssuedKey" } } } },
              "400": { "$ref": "#/components/responses/BadRequest" },
              "404": { "$ref": "#/components/responses/NotFound" }
            }
          }
        },
        "/api/v1/admin/keys/{key_id}": {
          "delete": {
            "tags": ["administração"],
            "summary": "Revoga uma chave",
            "description": "Efeito imediato: a revogação é conferida a cada requisição.",
            "parameters": [{ "name": "key_id", "in": "path", "required": true, "schema": { "type": "string" } }],
            "responses": {
              "200": { "description": "Revogada." },
              "404": { "$ref": "#/components/responses/NotFound" }
            }
          }
        },
        "/api/v1/admin/usage": {
          "get": {
            "tags": ["administração"],
            "summary": "Consumo de qualquer tenant",
            "parameters": [
              { "name": "tenant_id", "in": "query", "required": true, "schema": { "type": "string" } },
              { "$ref": "#/components/parameters/usageFrom" },
              { "$ref": "#/components/parameters/usageTo" },
              { "$ref": "#/components/parameters/usageDays" },
              { "$ref": "#/components/parameters/usageEvents" }
            ],
            "responses": {
              "200": { "description": "Totais e cotação.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UsageResponse" } } } },
              "400": { "$ref": "#/components/responses/BadRequest" },
              "404": { "$ref": "#/components/responses/NotFound" }
            }
          }
        },
        "/api/v1/codecs": {
          "get": {
            "tags": ["descoberta"],
            "summary": "Catálogo de codecs, transforms e modos de esforço",
            "description": "Permite montar chamadas sem hardcodar nomes nem faixas de nível.",
            "responses": { "200": { "description": "Capacidades do motor." } }
          }
        },
        "/api/v1/dictionaries": {
          "get": {
            "tags": ["descoberta"],
            "summary": "Dicionários treinados e em uso",
            "responses": { "200": { "description": "Estado dos dicionários." } }
          }
        },
        "/api/v1/stats": {
          "get": {
            "tags": ["operação"],
            "summary": "Números do motor",
            "description": "`process` são contadores desta instância; `lifetime` é o acumulado persistido em banco.",
            "responses": { "200": { "description": "Estatísticas." } }
          }
        },
        "/health": {
          "get": {
            "tags": ["operação"], "summary": "Liveness", "security": [],
            "description": "Não toca banco nem disco.",
            "responses": { "200": { "description": "Processo vivo." } }
          }
        },
        "/ready": {
          "get": {
            "tags": ["operação"], "summary": "Readiness", "security": [],
            "description": "Confere banco e vault.",
            "responses": {
              "200": { "description": "Pronto para receber tráfego." },
              "503": { "description": "Dependência indisponível." }
            }
          }
        },
        "/metrics": {
          "get": {
            "tags": ["operação"], "summary": "Métricas Prometheus", "security": [],
            "responses": { "200": { "description": "Exposição em texto.", "content": { "text/plain": { "schema": { "type": "string" } } } } }
          }
        }
      }
    }))
}
