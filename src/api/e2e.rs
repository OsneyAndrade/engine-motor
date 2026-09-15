use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::config::Config;
use crate::planner::Effort;
use crate::service::EngineState;

async fn motor(nome: &str, api_keys: Vec<String>) -> (Arc<EngineState>, std::path::PathBuf) {
    let raiz = std::env::temp_dir().join(format!(
        "syntra-e2e-{}-{}-{}",
        nome,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&raiz);
    std::fs::create_dir_all(&raiz).unwrap();

    let config = Config {
        vault_path: raiz.join("vault").to_string_lossy().to_string(),
        sled_index_path: raiz.join("index.sled").to_string_lossy().to_string(),
        dict_path: raiz.join("dicts").to_string_lossy().to_string(),
        db_url: "sqlite::memory:".to_string(),
        watch_dir: raiz.join("watch").to_string_lossy().to_string(),
        bind_addr: "127.0.0.1:0".to_string(),
        max_body_size_mb: 64,
        cors_origins: vec![],
        api_keys,
        compress_multiplier: 2,
        default_effort: Effort::Balanced,
        verify_on_write: true,
        dictionaries_enabled: true,
        redis_url: None,
        limits: Default::default(),
        watch_enabled: false,
        watch_delete_source: false,
    };

    let state = Arc::new(EngineState::new(config).await.expect("motor não subiu"));
    (state, raiz)
}

fn limpar(raiz: &std::path::Path) {
    let _ = std::fs::remove_dir_all(raiz);
}

async fn enviar(state: &Arc<EngineState>, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let resposta = super::router(state.clone()).oneshot(req).await.unwrap();
    let status = resposta.status();
    let corpo = to_bytes(resposta.into_body(), usize::MAX).await.unwrap();
    (status, corpo.to_vec())
}

async fn enviar_completo(
    state: &Arc<EngineState>,
    req: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let resposta = super::router(state.clone()).oneshot(req).await.unwrap();
    let status = resposta.status();
    let headers = resposta.headers().clone();
    let corpo = to_bytes(resposta.into_body(), usize::MAX).await.unwrap();
    (status, headers, corpo.to_vec())
}

fn json(corpo: &[u8]) -> serde_json::Value {
    serde_json::from_slice(corpo)
        .unwrap_or_else(|e| panic!("resposta não é JSON ({e}): {}", String::from_utf8_lossy(corpo)))
}

fn csv(linhas: usize) -> Vec<u8> {
    let mut s = String::from("id,data,regiao,produto,valor,status\n");
    for i in 0..linhas {
        s.push_str(&format!(
            "{},2026-0{}-{:02},sudeste,produto-{},{}.{:02},confirmado\n",
            i,
            (i % 9) + 1,
            (i % 28) + 1,
            i % 60,
            i * 17,
            i % 100
        ));
    }
    s.into_bytes()
}

fn aleatorio(n: usize) -> Vec<u8> {
    let mut x = 0x5DEECE66u32;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 9) as u8
        })
        .collect()
}

fn post(uri: &str, corpo: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .body(Body::from(corpo))
        .unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn ciclo_comprimir_reconstruir_devolve_bytes_identicos() {
    let (state, raiz) = motor("ciclo", vec![]).await;
    let original = csv(4000);

    let (status, headers, essencia) = enviar_completo(
        &state,
        post("/api/v1/compress?filename=vendas.csv&effort=max", original.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&essencia));

    let id = headers["x-syntra-id"].to_str().unwrap().to_string();
    assert_eq!(id.len(), 64, "id deveria ser o hash BLAKE3 em hex");
    assert_eq!(headers["x-syntra-verified"], "true");
    assert_eq!(headers["x-syntra-content-class"], "tabular");

    let economia: f64 = headers["x-syntra-savings-pct"].to_str().unwrap().parse().unwrap();
    assert!(economia > 80.0, "economia insuficiente em CSV: {economia}%");
    assert!(essencia.len() < original.len());

    let (status, headers, reconstruido) =
        enviar_completo(&state, post("/api/v1/decompress", essencia.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["x-syntra-fidelity-checked"], "true");
    assert_eq!(
        reconstruido, original,
        "reconstrução não é idêntica ao original"
    );

    let (status, _, do_vault) =
        enviar_completo(&state, get(&format!("/api/v1/objects/{id}/content"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(do_vault, original);

    let (status, _, do_vault_essencia) =
        enviar_completo(&state, get(&format!("/api/v1/objects/{id}/essence"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(do_vault_essencia, essencia);

    limpar(&raiz);
}

#[tokio::test]
async fn relatorio_json_descreve_o_plano_escolhido() {
    let (state, raiz) = motor("relatorio", vec![]).await;
    let (status, corpo) = enviar(
        &state,
        post(
            "/api/v1/compress?filename=vendas.csv&effort=max&response=json",
            csv(3000),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let j = json(&corpo);
    assert_eq!(j["content_class"], "tabular");
    assert_eq!(j["verified"], true);
    assert_eq!(j["persisted"], true);
    assert!(j["savings_pct"].as_f64().unwrap() > 80.0);
    assert!(j["candidates_tried"].as_u64().unwrap() > 1, "modo max deveria medir vários planos");
    assert!(!j["plan"]["label"].as_str().unwrap().is_empty());
    assert!(!j["reason"].as_str().unwrap().is_empty());
    assert!(j["links"]["content"].as_str().unwrap().contains("/content"));
    limpar(&raiz);
}

#[tokio::test]
async fn conteudo_repetido_e_deduplicado() {
    let (state, raiz) = motor("dedup", vec![]).await;
    let dados = csv(1500);

    let (_, primeiro) = enviar(
        &state,
        post("/api/v1/compress?filename=a.csv&response=json", dados.clone()),
    )
    .await;
    let p = json(&primeiro);
    assert_eq!(p["deduplicated"], false);
    assert_eq!(p["references"], 1);

    let (_, segundo) = enviar(
        &state,
        post("/api/v1/compress?filename=copia.csv&response=json", dados),
    )
    .await;
    let s = json(&segundo);
    assert_eq!(s["deduplicated"], true, "segundo envio deveria deduplicar");
    assert_eq!(s["id"], p["id"], "mesmo conteúdo deve ter o mesmo id");
    assert_eq!(s["references"], 2);

    assert_eq!(state.vault.len(), 1);
    limpar(&raiz);
}

#[tokio::test]
async fn conteudo_incompressivel_nunca_cresce() {
    let (state, raiz) = motor("incompressivel", vec![]).await;
    let dados = aleatorio(300_000);

    let (status, corpo) = enviar(
        &state,
        post(
            "/api/v1/compress?filename=chave.bin&effort=max&response=json",
            dados.clone(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let j = json(&corpo);
    assert_eq!(j["essence_size"].as_u64().unwrap(), dados.len() as u64);
    assert_eq!(j["plan"]["codec"], "stored");
    assert!(j["savings_pct"].as_f64().unwrap() >= 0.0, "economia não pode ser negativa");

    let id = j["id"].as_str().unwrap();
    let (_, _, volta) =
        enviar_completo(&state, get(&format!("/api/v1/objects/{id}/content"))).await;
    assert_eq!(volta, dados);
    limpar(&raiz);
}

#[tokio::test]
async fn analise_compara_planos_sem_gravar() {
    let (state, raiz) = motor("analise", vec![]).await;
    let (status, corpo) = enviar(
        &state,
        post("/api/v1/analyze?filename=vendas.csv&effort=max", csv(2000)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let j = json(&corpo);
    assert_eq!(j["content_class"], "tabular");
    assert_eq!(j["detected_columns"], 6);
    let candidatos = j["candidates"].as_array().unwrap();
    assert!(candidatos.len() > 3, "modo max deveria comparar vários planos");

    let tamanhos: Vec<u64> = candidatos
        .iter()
        .filter(|c| c["error"].is_null())
        .map(|c| c["output_size"].as_u64().unwrap())
        .collect();
    assert!(tamanhos.windows(2).all(|w| w[0] <= w[1]), "candidatos fora de ordem");
    assert_eq!(j["recommended"], candidatos[0]["plan"]);

    assert_eq!(state.vault.len(), 0, "analyze não deveria gravar");
    limpar(&raiz);
}

#[tokio::test]
async fn esforco_maior_comprime_mais() {
    let (state, raiz) = motor("esforco", vec![]).await;
    let dados = csv(5000);

    let mut tamanhos = Vec::new();
    for esforco in ["fast", "balanced", "max"] {
        let (status, _, essencia) = enviar_completo(
            &state,
            post(
                &format!("/api/v1/compress?filename=v.csv&persist=false&effort={esforco}"),
                dados.clone(),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        tamanhos.push((esforco, essencia.len()));
    }

    let fast = tamanhos[0].1;
    let max = tamanhos[2].1;
    assert!(
        max < fast,
        "max ({max}) deveria comprimir mais que fast ({fast}): {tamanhos:?}"
    );
    limpar(&raiz);
}

#[tokio::test]
async fn codec_imposto_e_respeitado_e_validado() {
    let (state, raiz) = motor("imposto", vec![]).await;
    let dados = csv(1000);

    let (status, corpo) = enviar(
        &state,
        post(
            "/api/v1/compress?filename=v.csv&codec=xz&level=9&response=json",
            dados.clone(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&corpo);
    assert_eq!(j["plan"]["codec"], "xz");
    assert_eq!(j["plan"]["level"], 9);
    assert_eq!(j["candidates_tried"], 1, "codec imposto não deveria medir candidatos");

    let (status, corpo) = enviar(
        &state,
        post("/api/v1/compress?codec=bzip2&level=99", dados.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&corpo)["error"]["code"], "invalid_level");

    let (status, corpo) = enviar(&state, post("/api/v1/compress?codec=magia", dados)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json(&corpo)["error"]["code"], "unsupported_codec");
    limpar(&raiz);
}

#[tokio::test]
async fn essencia_corrompida_e_recusada_em_vez_de_devolver_dado_ruim() {
    let (state, raiz) = motor("corrompida", vec![]).await;

    let (_, _, mut essencia) = enviar_completo(
        &state,
        post("/api/v1/compress?filename=v.csv&persist=false", csv(2000)),
    )
    .await;

    let meio = essencia.len() / 2;
    essencia[meio] ^= 0xFF;

    let (status, corpo) = enviar(&state, post("/api/v1/decompress", essencia)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let j = json(&corpo);
    assert_eq!(j["error"]["code"], "envelope_corrupted");
    assert!(j["error"]["hint"].is_string(), "erro deveria trazer dica acionável");
    limpar(&raiz);
}

#[tokio::test]
async fn identificador_malicioso_e_rejeitado() {
    let (state, raiz) = motor("traversal", vec![]).await;

    for id in ["zzz", "..", "abc", &"f".repeat(63), "0123456789abcdefg"] {
        let (status, corpo) = enviar(&state, get(&format!("/api/v1/objects/{id}"))).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "id {id:?} deveria ser rejeitado"
        );
        assert_eq!(json(&corpo)["error"]["code"], "invalid_object_id");
    }

    let (status, corpo) = enviar(&state, get(&format!("/api/v1/objects/{}", "a".repeat(64)))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(json(&corpo)["error"]["code"], "not_found");
    limpar(&raiz);
}

#[tokio::test]
async fn autenticacao_por_api_key() {
    let (state, raiz) = motor("auth", vec!["chave-secreta".to_string()]).await;

    let (status, corpo) = enviar(&state, post("/api/v1/compress", csv(100))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json(&corpo)["error"]["code"], "unauthorized");

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/compress")
        .header("x-api-key", "chave-errada")
        .body(Body::from(csv(100)))
        .unwrap();
    assert_eq!(enviar(&state, req).await.0, StatusCode::UNAUTHORIZED);

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/compress?persist=false")
        .header("x-api-key", "chave-secreta")
        .body(Body::from(csv(100)))
        .unwrap();
    assert_eq!(enviar(&state, req).await.0, StatusCode::OK);

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/compress?persist=false")
        .header("authorization", "Bearer chave-secreta")
        .body(Body::from(csv(100)))
        .unwrap();
    assert_eq!(enviar(&state, req).await.0, StatusCode::OK);

    assert_eq!(enviar(&state, get("/health")).await.0, StatusCode::OK);
    assert_eq!(enviar(&state, get("/metrics")).await.0, StatusCode::OK);
    assert_eq!(
        enviar(&state, get("/api/v1/openapi.json")).await.0,
        StatusCode::OK
    );
    limpar(&raiz);
}

#[tokio::test]
async fn descoberta_de_capacidades_e_contrato() {
    let (state, raiz) = motor("descoberta", vec![]).await;

    let (status, corpo) = enviar(&state, get("/api/v1/codecs")).await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&corpo);
    let nomes: Vec<&str> = j["codecs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    for esperado in ["stored", "lz4", "deflate", "zstd", "bzip2", "brotli", "xz"] {
        assert!(nomes.contains(&esperado), "codec {esperado} ausente do catálogo");
    }
    assert_eq!(j["transforms"].as_array().unwrap().len(), 4);
    assert_eq!(j["efforts"].as_array().unwrap().len(), 3);

    let (status, corpo) = enviar(&state, get("/api/v1/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);
    let doc = json(&corpo);
    assert_eq!(doc["openapi"], "3.0.3");
    for rota in [
        "/api/v1/compress",
        "/api/v1/decompress",
        "/api/v1/analyze",
        "/api/v1/objects/{id}",
        "/api/v1/objects/{id}/content",
        "/metrics",
    ] {
        assert!(doc["paths"][rota].is_object(), "rota {rota} ausente do OpenAPI");
    }
    limpar(&raiz);
}

#[tokio::test]
async fn remocao_respeita_contador_de_referencias() {
    let (state, raiz) = motor("remocao", vec![]).await;
    let dados = csv(800);

    let (_, corpo) = enviar(
        &state,
        post("/api/v1/compress?filename=a.csv&response=json", dados.clone()),
    )
    .await;
    let id = json(&corpo)["id"].as_str().unwrap().to_string();
    enviar(
        &state,
        post("/api/v1/compress?filename=b.csv&response=json", dados),
    )
    .await;

    let (status, corpo) = enviar(
        &state,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/objects/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&corpo);
    assert_eq!(j["removed"], false);
    assert_eq!(j["remaining_references"], 1);
    assert_eq!(
        enviar(&state, get(&format!("/api/v1/objects/{id}"))).await.0,
        StatusCode::OK,
        "objeto não deveria ter saído do disco ainda"
    );

    let (_, corpo) = enviar(
        &state,
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/v1/objects/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(json(&corpo)["removed"], true);
    assert_eq!(
        enviar(&state, get(&format!("/api/v1/objects/{id}"))).await.0,
        StatusCode::NOT_FOUND
    );
    limpar(&raiz);
}

#[tokio::test]
async fn rotas_legadas_continuam_funcionando() {
    let (state, raiz) = motor("legado", vec![]).await;
    let original = csv(1200);

    let req = Request::builder()
        .method("POST")
        .uri("/process")
        .header("x-filename", "antigo.csv")
        .body(Body::from(original.clone()))
        .unwrap();
    let (status, headers, essencia) = enviar_completo(&state, req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["x-syn-filename"].to_str().unwrap().ends_with(".syntra"));

    let (status, _, volta) = enviar_completo(&state, post("/reconstruct", essencia)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(volta, original);

    let req = Request::builder()
        .method("POST")
        .uri("/api/sim/process")
        .header("x-filename", "sim.csv")
        .body(Body::from(csv(900)))
        .unwrap();
    let (status, corpo) = enviar(&state, req).await;
    assert_eq!(status, StatusCode::OK);
    let sim = json(&corpo);
    for campo in [
        "original_name",
        "original_size",
        "reduced_size",
        "savings_pct",
        "processing_time_ms",
        "download_id",
    ] {
        assert!(!sim[campo].is_null(), "campo {campo} ausente em /api/sim/process");
    }

    let download_id = sim["download_id"].as_str().unwrap();
    let (status, _, _) =
        enviar_completo(&state, get(&format!("/api/sim/download/{download_id}"))).await;
    assert_eq!(status, StatusCode::OK);

    let (status, corpo) = enviar(&state, get("/api/files")).await;
    assert_eq!(status, StatusCode::OK);
    let lista = json(&corpo);
    let primeiro = &lista.as_array().unwrap()[0];
    for campo in [
        "name",
        "original_name",
        "original_size",
        "essence_size",
        "savings_pct",
        "processing_time_ms",
        "created_at",
    ] {
        assert!(!primeiro[campo].is_null(), "campo {campo} ausente em /api/files");
    }

    let (status, corpo) = enviar(&state, get("/stats")).await;
    assert_eq!(status, StatusCode::OK);
    let s = json(&corpo);
    assert!(!s["roi"]["total_bytes_in"].is_null());
    assert!(!s["roi"]["efficiency_pct"].is_null());
    assert!(!s["system"]["cpu_usage_pct"].is_null());
    assert!(!s["latency"]["sum_ms"].is_null());
    assert!(!s["processing_strategies"]["ultra_fast"].is_null());
    limpar(&raiz);
}

#[tokio::test]
async fn arquivo_ja_comprimido_e_reconhecido_e_nao_reprocessado_em_vao() {
    let (state, raiz) = motor("precomprimido", vec![]).await;

    let mut zip = b"PK\x03\x04".to_vec();
    zip.extend_from_slice(&aleatorio(120_000));

    let (status, corpo) = enviar(
        &state,
        post(
            "/api/v1/compress?filename=pacote.zip&response=json&effort=max",
            zip.clone(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&corpo);
    assert_eq!(j["content_class"], "pre_compressed");
    assert!(j["essence_size"].as_u64().unwrap() <= zip.len() as u64);
    limpar(&raiz);
}

#[tokio::test]
async fn inspect_le_metadados_sem_reconstruir() {
    let (state, raiz) = motor("inspect", vec![]).await;
    let (_, _, essencia) = enviar_completo(
        &state,
        post(
            "/api/v1/compress?filename=v.csv&persist=false&effort=max",
            csv(1500),
        ),
    )
    .await;

    let (status, corpo) = enviar(&state, post("/api/v1/inspect", essencia)).await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&corpo);
    assert_eq!(j["original_name"], "v.csv");
    assert_eq!(j["content_class"], "tabular");
    assert_eq!(j["checksum_ok"], true);
    assert_eq!(j["file_hash"].as_str().unwrap().len(), 64);
    assert!(j["metrics"]["candidates_tried"].as_u64().unwrap() > 1);
    limpar(&raiz);
}

#[tokio::test]
async fn corpo_vazio_gera_erro_de_cliente() {
    let (state, raiz) = motor("vazio", vec![]).await;

    for rota in ["/api/v1/decompress", "/api/v1/analyze", "/api/v1/inspect"] {
        let (status, corpo) = enviar(&state, post(rota, Vec::new())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "rota {rota}");
        assert_eq!(json(&corpo)["error"]["code"], "empty_body");
    }

    let (status, _, essencia) = enviar_completo(
        &state,
        post("/api/v1/compress?filename=vazio.bin&persist=false", Vec::new()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, volta) = enviar_completo(&state, post("/api/v1/decompress", essencia)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(volta.is_empty());
    limpar(&raiz);
}

#[tokio::test]
async fn id_de_requisicao_e_propagado() {
    let (state, raiz) = motor("reqid", vec![]).await;

    let (_, headers, _) = enviar_completo(&state, get("/health")).await;
    assert!(headers.contains_key("x-request-id"));

    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .header("x-request-id", "trace-abc-123")
        .body(Body::empty())
        .unwrap();
    let (_, headers, _) = enviar_completo(&state, req).await;
    assert_eq!(headers["x-request-id"], "trace-abc-123");
    limpar(&raiz);
}

#[tokio::test]
async fn metricas_e_estatisticas_refletem_o_processamento() {
    let (state, raiz) = motor("metricas", vec![]).await;
    enviar(
        &state,
        post("/api/v1/compress?filename=v.csv&effort=max", csv(2000)),
    )
    .await;

    let (status, corpo) = enviar(&state, get("/metrics")).await;
    assert_eq!(status, StatusCode::OK);
    let prom = String::from_utf8_lossy(&corpo);
    assert!(prom.contains("syntra_items_total 1"));
    assert!(prom.contains("syntra_codec_usage_total{codec="));
    assert!(prom.contains("syntra_processing_latency_ms_bucket{le=\"+Inf\"} 1"));
    assert!(prom.contains("syntra_verify_failures_total 0"));

    let (status, corpo) = enviar(&state, get("/api/v1/stats")).await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&corpo);
    assert_eq!(j["process"]["throughput"]["items_processed"], 1);
    assert_eq!(j["process"]["throughput"]["verify_failures"], 0);
    assert_eq!(j["lifetime"]["items_processed"], 1);
    assert_eq!(j["vault"]["objects_indexed"], 1);
    assert_eq!(j["config"]["verify_on_write"], true);
    limpar(&raiz);
}

#[tokio::test]
async fn array_numerico_usa_transform_de_pre_processamento() {
    let (state, raiz) = motor("numerico", vec![]).await;

    let mut dados = Vec::new();
    for i in 0..40_000u32 {
        dados.extend_from_slice(&(18.5f32 + i as f32 * 0.0005).to_le_bytes());
    }

    let (status, corpo) = enviar(
        &state,
        post(
            "/api/v1/compress?filename=serie.dat&effort=max&response=json",
            dados.clone(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let j = json(&corpo);
    assert_eq!(j["content_class"], "numeric_binary");
    assert_ne!(
        j["plan"]["transforms"], "none",
        "deveria ter aplicado transform: plano={}",
        j["plan"]["label"]
    );
    assert!(j["savings_pct"].as_f64().unwrap() > 50.0);

    let id = j["id"].as_str().unwrap();
    let (_, _, volta) =
        enviar_completo(&state, get(&format!("/api/v1/objects/{id}/content"))).await;
    assert_eq!(volta, dados);
    limpar(&raiz);
}
