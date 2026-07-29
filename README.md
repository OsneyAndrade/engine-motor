# Syntra Engine

> **Motor de Geração de Essência de Alta Performance — Escrito em Rust**

O Syntra Engine é uma infraestrutura de **destilação de dados adaptativa** que reduz o espaço de armazenamento em até 80% enquanto mantém integridade total e reconstrução bit-perfect dos dados originais. Projetado para rodar dentro de datacenters on-premise e como API para integração com storages em cloud (Amazon S3, Google Cloud Storage, Azure Blob).

---

## 📋 Índice

- [Visão Geral](#visão-geral)
- [Arquitetura](#arquitetura)
- [Componentes](#componentes)
- [Como Funciona](#como-funciona)
- [Instalação](#instalação)
- [Configuração](#configuração)
- [Docker](#docker)
- [API Endpoints](#api-endpoints)
- [Dashboard](#dashboard)
- [Desenvolvimento](#desenvolvimento)
- [Performance](#performance)

---

## 📖 Visão Geral

### O que é o Syntra Engine?

O **Syntra Engine** é um motor que processa arquivos e cria uma **essência** — uma versão otimizada que ocupa muito menos espaço, mas pode ser reconstruída exatamente como o original a qualquer momento.

Ele opera em dois modelos:

| Modo de Deploy | Descrição |
|----------------|-----------|
| **Datacenter On-Premise** | Rodar dentro do servidor do cliente, processando arquivos locais e via Watch Directory |
| **API Cloud** | Exposto como endpoint HTTP para integração com pipelines que armazenam em S3, GCS, Azure Blob, etc. |

### Características Principais

| Característica | Descrição |
|----------------|-----------|
| **Geração de Essência Adaptativa** | Seleciona automaticamente o melhor algoritmo (Zstd, LZ4) baseado na entropia dos dados |
| **Modelos de Contexto (Dicionários)** | Treina modelos otimizados por MIME-type para eficiência crescente |
| **Vault com Sharding** | Armazena essências em estrutura de 2 níveis de hash para alta performance |
| **Monitoramento** | Métricas em tempo real via Prometheus e dashboard web embutido |
| **Auto-Ingestão** | Monitora diretório e processa novos arquivos automaticamente |
| **Multi-Banco** | Suporta SQLite (dev) ou PostgreSQL (produção) para metadados |
| **Cluster-Aware** | Sincroniza Modelos de Contexto via Redis para aprendizado distribuído |

### Por que usar o Syntra?

- **Economia de 40–80%** no espaço de armazenamento
- **Processamento paralelo** via Rayon thread pool
- **Zero perdas** — reconstrução bit-perfect com verificação BLAKE3
- **Baixo overhead** — geração de essência em streaming sem carregar tudo em memória
- **Auto-aprendizado** — quanto mais arquivos do mesmo tipo, maior a eficiência

---

## 🏗️ Arquitetura

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Syntra Engine Architecture                      │
└─────────────────────────────────────────────────────────────────────┘

┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│   Clientes HTTP  │────▶│   HTTP API       │────▶│    Watcher       │
│  (Apps, S3, GCS) │ POST│   (Axum 0.7)     │     │  (notify + poll) │
└──────────────────┘     └────────┬─────────┘     └────────┬─────────┘
                                   │                         │
                                   ▼                         ▼
                          ┌──────────────────────────────────┐
                          │          Request Router           │
                          │  /process  /reconstruct  /api/*  │
                          └──────────────────────────────────┘
                                           │
                          ┌────────────────┼────────────────┐
                          ▼                ▼                ▼
                ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
                │  Adaptive   │  │  Compress   │  │    Vault    │
                │  Analyzer   │  │   Module    │  │   Manager   │
                │  (entropy)  │  │ (Zstd/LZ4)  │  │  (Sled)     │
                └──────┬──────┘  └──────┬──────┘  └──────┬──────┘
                       │                │                  │
                       └────────────────┼──────────────────┘
                                        ▼
                          ┌─────────────────────────────┐
                          │         Storage Layer        │
                          │   essence_vault/             │
                          │     ├── ab/cd/hash.syntra    │
                          │     └── ef/12/hash.syntra    │
                          └─────────────────────────────┘
                                        │
                          ┌─────────────┼─────────────┐
                          ▼             ▼             ▼
                ┌─────────────┐ ┌────────────┐ ┌───────────┐
                │  Monitor DB │ │  Metrics   │ │ Dashboard │
                │(SQLite/PG)  │ │(Prometheus)│ │ (HTML/JS) │
                └─────────────┘ └────────────┘ └───────────┘
```

---

## 🧩 Componentes

### 1. Módulo de Análise Adaptativa ([`adaptive.rs`](src/adaptive.rs))

Analisa os dados de entrada e decide automaticamente qual algoritmo usar.

**Como funciona:**
- Calcula a **Entropia Shannon** dos primeiros 1MB dos dados
- Detecta o tipo de arquivo via assinatura (MIME type)
- Decide a estratégia ótima:

| Entropia | Tipo | Tamanho | Algoritmo |
|----------|------|---------|-----------|
| ≥ 7.8 | qualquer | qualquer | **Passthrough** (não processa) |
| < 4.5 | qualquer | qualquer | **LZ4** (ultra rápido) |
| < 4.8 | binário/outros | qualquer | **ZstdBalanced** (-5) |
| texto estruturado | JSON/XML/CSV | > 50MB | **LZ4** |
| texto estruturado | JSON/XML/CSV | ≤ 50MB | **ZstdBalanced** (-5) |
| 4.5–7.8 | outros | qualquer | **LZ4** |
| fallback | qualquer | qualquer | **ZstdFast** (-7) |

### 2. Módulo de Geração de Essência ([`compress.rs`](src/compress.rs))

Implementa os algoritmos de geração de essência:

| Algoritmo | Velocidade | Razão Típica | Uso Ideal |
|-----------|------------|--------------|-----------|
| **ZstdFast** | Muito rápida | 2–3x | Arquivos variados (fallback) |
| **ZstdBalanced** | Rápida | 3–6x | Texto estruturado (JSON, XML, CSV) |
| **LZ4** | Ultra rápida (>500 MB/s) | 2–2.5x | Dados repetitivos, arquivos grandes |
| **Passthrough** | Instantânea | 1x | Dados já comprimidos/criptografados |

### 3. Módulo de Vault ([`vault.rs`](src/vault.rs))

Gerencia o armazenamento de arquivos processados.

**Estrutura de diretórios (Sharding):**
```
essence_vault/
├── ab/                               # Primeiros 2 bytes do hash BLAKE3
│   └── cd/                           # Próximos 2 bytes do hash
│       └── abcd...1234_1707839.syntra # {hash}_{timestamp_nanos}.syntra
├── 12/
│   └── 34/
│       └── 1234...5678_1707840.syntra
└── ...
```

**Formato do nome do arquivo:** `{hash_blake3}_{timestamp_nanos}.syntra`
- O timestamp garante que arquivos com o mesmo conteúdo tenham nomes únicos
- Excluir um arquivo não afeta outros com o mesmo hash
- Sharding em 2 níveis melhora performance em infra com milhões de arquivos

### 4. Módulo de Dicionários ([`dictionary.rs`](src/dictionary.rs))

Treina Modelos de Contexto (dicionários Zstd) para maior eficiência por tipo de dado.

**Após 10+ arquivos do mesmo MIME-type:**
- Coleta amostras de todos os arquivos processados
- Treina um dicionário específico para aquele tipo
- As próximas essências daquele tipo ganham **20–50% a mais de compressão**

**Tipos de dicionário suportados:**
- `application/json` — APIs, exports de banco de dados
- `text/csv` — planilhas, relatórios
- `text/plain` — logs de sistema, telemetria
- `image/jpeg` — fotos com padrão similar

**Cluster Mode:** Dicionários são sincronizados via Redis para que todos os nós aprendam juntos.

### 5. Monitor DB ([`db_monitor.rs`](src/db_monitor.rs))

Rastreia todos os arquivos processados para auditoria e métricas de ROI.

**Schema (SQLite/PostgreSQL):**
```sql
CREATE TABLE processed_files (
    id TEXT PRIMARY KEY,           -- Identificador único (hash + timestamp)
    original_name TEXT NOT NULL,   -- Nome original do arquivo
    raw_size INTEGER NOT NULL,     -- Tamanho antes da destilação
    compressed_size INTEGER NOT NULL, -- Tamanho da essência
    savings_pct REAL NOT NULL,     -- % economizado
    algorithm TEXT NOT NULL,       -- Algoritmo utilizado
    duration_ms REAL NOT NULL,     -- Tempo de processamento
    timestamp TIMESTAMP DEFAULT NOW()
);
```

**Operações principais:**
- `register_file()` — Registra arquivo processado
- `list_files()` — Lista com paginação e filtro
- `get_stats()` — Agrega métricas (COUNT, SUM, AVG, economia total)
- `delete_file()` — Remove por ID único
- `get_file_by_hash_and_timestamp()` — Busca para download correto

### 6. Módulo de Métricas ([`metrics.rs`](src/metrics.rs))

Coleta métricas em tempo real usando contadores atômicos (lock-free).

**Métricas exportadas (formato Prometheus):**
```prometheus
syntra_bytes_in_total           # Bytes de entrada recebidos
syntra_essence_bytes_total      # Bytes de saída (essências geradas)
syntra_essence_ratio            # Razão global de destilação
syntra_items_total              # Total de arquivos processados
syntra_processing_latency_ms    # Histograma de latência por arquivo
syntra_model_usage{model="..."}  # Uso por algoritmo (ZstdFast, LZ4, etc.)
```

### 7. Módulo de Watcher ([`watcher.rs`](src/watcher.rs))

Monitora diretório para ingestão automática de arquivos.

**Três mecanismos de detecção:**
1. **Notify** — Eventos nativos do sistema de arquivos (inotify no Linux)
2. **Polling** — Verificação periódica a cada 2 segundos
3. **Batch Drain** — Processamento em lote a cada 500ms

**Fluxo:**
1. Detecta novo arquivo em `watch_in/`
2. Aguarda arquivo estar completamente escrito
3. Lê, processa e salva essência no vault
4. Remove arquivo original de `watch_in/`
5. Registra no banco de monitoramento

### 8. HTTP Handlers ([`handlers.rs`](src/handlers.rs))

Implementa todos os endpoints da API REST, incluindo upload, download, listagem, deleção e métricas.

---

## 🔄 Como Funciona

### Geração de Essência (Flow)

```
1. Upload (POST /process)
   │
   ├─► Lê bytes do arquivo via streaming
   ├─► Detecta MIME type (infer)
   ├─► Calcula hash BLAKE3
   │
2. Análise Adaptativa
   │
   ├─► Calcula entropia Shannon (primeiros 1MB)
   ├─► Seleciona algoritmo ideal
   ├─► Verifica se há Modelo de Contexto treinado
   │
3. Destilação
   │
   ├─► Aplica algoritmo selecionado
   ├─► (opcional) Usa dicionário para ganho extra
   │
4. Armazenamento
   │
   ├─► Salva no vault (sharded por hash)
   ├─► Registra no banco (nome, tamanho, algoritmo, saving%)
   ├─► Atualiza métricas atômicas
   │
5. Resposta
   │
   └─► Retorna essência (.syntra) ao cliente
```

### Reconstrução (Flow)

```
1. Upload da essência (POST /reconstruct)
   │
   ├─► Deserializa metadados (Protobuf)
   ├─► Identifica algoritmo usado
   │
2. Destilação Inversa
   │
   ├─► Aplica algoritmo inverso (Zstd/LZ4 decompress)
   ├─► (se aplicável) Usa Modelo de Contexto
   │
3. Verificação
   │
   ├─► Recalcula hash BLAKE3 do dado reconstruído
   ├─► Compara com hash original armazenado
   │
4. Resposta
   │
   └─► Retorna arquivo original exatamente como era
```

---

## 🚀 Instalação

### Pré-requisitos

- **Rust** 1.75+ (para desenvolvimento local)
- **Docker** e **Docker Compose** (para produção — recomendado)
- **PostgreSQL** 14+ ou **SQLite** 3+
- **Redis** (opcional — para Cluster Mode)

### Desenvolvimento Local

```bash
# Clone o repositório
git clone <repo-url>
cd syntra-rust/engine

# Configure o banco
export SYNTRA_DB_URL="sqlite://monitoring.db"
# OU PostgreSQL
export SYNTRA_DB_URL="postgres://user:pass@localhost:5432/syntra"

# Build e run
cargo build --release
cargo run --release
```

O servidor estará disponível em `http://localhost:3002`

---

## ⚙️ Configuração

Todas as configurações são feitas via **variáveis de ambiente**:

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `SYNTRA_VAULT_PATH` | `essence_vault` | Diretório para armazenar essências |
| `SYNTRA_SLED_INDEX` | `metadata_index.sled` | Caminho do índice Sled |
| `SYNTRA_DB_URL` | `postgres://...` | URL do banco de monitoramento |
| `SYNTRA_WATCH_DIR` | `watch_in` | Diretório monitorado para auto-ingestão |
| `SYNTRA_BIND_ADDR` | `0.0.0.0:3002` | Endereço do servidor HTTP |
| `SYNTRA_MAX_BODY_MB` | `4096` | Tamanho máximo de upload (MB) |
| `SYNTRA_COMPRESS_MULTIPLIER` | `4` | Multiplicador I/O vs CPU para semáforo |
| `RAYON_NUM_THREADS` | `(auto)` | Threads do Rayon (vazio = todos os cores) |
| `RUST_LOG` | `engine=debug` | Nível de log |

### Exemplo de configuração para produção

```bash
# PostgreSQL
export SYNTRA_DB_URL="postgres://syntra:senha@db.example.com:5432/production"

# Caminhos de armazenamento
export SYNTRA_VAULT_PATH="/data/essence_vault"
export SYNTRA_WATCH_DIR="/data/inbox"

# Performance
export RAYON_NUM_THREADS="16"
export SYNTRA_COMPRESS_MULTIPLIER="4"

# Servidor
export SYNTRA_BIND_ADDR="0.0.0.0:3002"
export SYNTRA_MAX_BODY_MB="10240"  # 10GB max upload
```

---

## 🐳 Docker

### Docker Compose (Recomendado para Produção)

```yaml
# docker-compose.prod.yml
services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: syntra
      POSTGRES_PASSWORD: syntra123
      POSTGRES_DB: syntra_monitoring
    volumes:
      - postgres_data:/var/lib/postgresql/data

  syntra:
    build: .
    ports:
      - "3002:3002"
    environment:
      SYNTRA_DB_URL: postgres://syntra:syntra123@postgres:5432/syntra_monitoring
      SYNTRA_VAULT_PATH: /data/essence_vault
      SYNTRA_WATCH_DIR: /data/watch_in
    volumes:
      - syntra_vault:/data/essence_vault
      - syntra_inbox:/data/watch_in
    depends_on:
      - postgres
```

### Comandos Docker

```bash
# Produção
docker-compose -f docker-compose.prod.yml up -d

# Desenvolvimento
docker-compose up --build

# Logs
docker-compose logs -f syntra

# Parar
docker-compose down

# Parar e remover volumes
docker-compose down -v
```

### Multi-Stage Dockerfile

```dockerfile
# Stage 1: Builder (Rust compilação)
FROM rust:1.88-slim-bookworm AS builder
# Compila dependências em cache separadamente
# Build final com --release (LTO fat, opt-level 3)

# Stage 2: Runtime (mínimo)
FROM debian:bookworm-slim
# Apenas o binário compilado + static/
# Executa como usuário não-root (syntra)
```

### Auto-Ingestão via Watch Directory

```bash
# Copie os arquivos para processar
cp /meus/arquivos/*.json /data/watch_in/

# O Watcher irá:
# 1. Detectar os novos arquivos
# 2. Processar e gerar essências
# 3. Salvar no vault
# 4. Deletar os originais
```

---

## 🌐 API Endpoints

### Health & Status

#### `GET /health`
```json
{ "status": "healthy", "version": "0.3.0" }
```

#### `GET /stats`
```json
{
  "files_processed": 1234,
  "total_bytes_in": 5368709120,
  "total_bytes_out": 1875648762,
  "savings_pct": 65.1,
  "avg_duration_ms": 234.5,
  "zstd_count": 856,
  "lz4_count": 312,
  "passthrough_count": 66
}
```

#### `GET /metrics`
Exporta métricas em formato Prometheus (`text/plain`).

---

### Processamento

#### `POST /process`
Processa um arquivo e retorna a essência `.syntra`.

**Headers:**
- `x-filename: documento.pdf` (opcional — para preservar nome original)
- `Content-Type: application/octet-stream`

**Response:** Binário `.syntra`

```bash
curl -X POST http://localhost:3002/process \
  -H "x-filename: relatorio.json" \
  -H "Content-Type: application/octet-stream" \
  --data-binary @relatorio.json \
  -o relatorio.syntra
```

#### `POST /reconstruct`
Reconstrói o arquivo original a partir da essência.

```bash
curl -X POST http://localhost:3002/reconstruct \
  -H "Content-Type: application/octet-stream" \
  --data-binary @relatorio.syntra \
  -o relatorio_reconstruido.json
```

---

### Gerenciamento de Arquivos

#### `GET /api/files`
Lista todos os arquivos processados (paginado).

**Query Params:** `page`, `limit`, `filter`

```json
{
  "files": [
    {
      "name": "abc123...456_1707839234567890123.syntra",
      "original_name": "relatorio.json",
      "size": 12345,
      "created_at": "2024-01-15T10:30:00Z",
      "processing_time_ms": 234.5,
      "algorithm": "ZstdBalanced"
    }
  ],
  "total": 1234,
  "page": 1,
  "limit": 50
}
```

#### `GET /api/files/:filename`
Download de uma essência específica.

```bash
curl -O http://localhost:3002/api/files/abc123...456_1707839234567890123.syntra
```

#### `DELETE /api/files/:filename`
Remove uma essência do vault e do banco.

```json
{
  "success": true,
  "message": "Arquivo deletado com sucesso",
  "id": "abc123...456_1707839234567890123.syntra"
}
```

---

## 📊 Dashboard

Disponível em `/dashboard`:

- **Cards de Métricas** — Arquivos processados, economia acumulada, uptime
- **Gráficos** — Distribuição de algoritmos por uso
- **Tabela de Arquivos** — Lista paginada com busca, download e deleção
- **Simulador** — Upload de teste com resultado ao vivo

---

## 🛠️ Desenvolvimento

### Estrutura do Projeto

```
engine/
├── src/
│   ├── main.rs           # Entry point, setup do servidor HTTP
│   ├── config.rs         # Configurações (env vars)
│   ├── adaptive.rs       # Análise de entropia e seleção de estratégia
│   ├── compress.rs       # Algoritmos de geração de essência
│   ├── vault.rs          # Gerenciamento de armazenamento sharded
│   ├── dictionary.rs     # Treinamento de Modelos de Contexto
│   ├── db_monitor.rs     # Operações de banco (SQLite/PostgreSQL)
│   ├── metrics.rs        # Métricas Prometheus (atômicos lock-free)
│   ├── watcher.rs        # File watcher (notify + polling)
│   └── handlers.rs       # HTTP handlers (Axum routes)
├── proto/                # Protocol Buffers definitions
├── static/
│   └── dashboard.html    # Dashboard UI (HTML + JS vanilla)
├── Cargo.toml            # Dependências e perfis de build
├── Dockerfile            # Multi-stage build
└── docker-compose.yml    # Orquestração de serviços
```

### Build & Test

```bash
# Hot-reload para desenvolvimento
cargo install cargo-watch
cargo watch -x run

# Testes
cargo test

# Benchmark
cargo bench

# Release otimizado (LTO fat, opt-level 3)
cargo build --release
```

### Logs

```bash
RUST_LOG=debug cargo run      # Verboso
RUST_LOG=info cargo run       # Produção
RUST_LOG=engine::vault=debug  # Módulo específico
```

---

## 📈 Performance

### Benchmarks Típicos

| Tipo de Arquivo | Tamanho | Tempo | Razão | Algoritmo |
|-----------------|---------|-------|-------|-----------|
| Log text | 100 MB | 0.8s | 5.2x | ZstdBalanced |
| JSON | 10 MB | 0.15s | 4.1x | ZstdBalanced |
| JPEG | 5 MB | 0.02s | 1.0x | Passthrough |
| PDF | 50 MB | 1.2s | 2.3x | LZ4 |
| XML | 25 MB | 0.4s | 6.8x | ZstdBalanced |

### Otimizações de Build (Perfil Release)

```toml
[profile.release]
opt-level     = 3        # Máxima otimização de código
lto           = "fat"    # Link-time optimization completa
codegen-units = 1        # Melhor otimização entre módulos
strip         = true     # Remove símbolos de debug do binário
panic         = "abort"  # Sem unwinding (menor e mais rápido)
```

### Otimizações de Runtime

- **Streaming**: Processa chunks sem carregar arquivo completo em memória
- **Paralelismo Rayon**: Todos os cores do CPU em uso para CPU-bound
- **Async Tokio**: I/O não-bloqueante para milhares de conexões simultâneas
- **mimalloc**: Alocador de memória otimizado para multi-core
- **Atômicos**: Métricas completamente lock-free
- **Sharding**: Vault distribuído em diretórios evita hotspot

---

## 📄 Licença

Este projeto é propriedade privada. Todos os direitos reservados.

---

*Syntra Engine — Engineered for Extreme Scale*
