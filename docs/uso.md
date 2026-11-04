# Guia de uso e operação

Para quem vai subir, operar e consumir o Syntra Engine. O contrato da API, as garantias e o modelo de cobrança estão no [README](../README.md); aqui está o **ciclo de uso**: subir, comprimir, restaurar, medir, fazer backup e conhecer os limites.

Tudo o que está marcado como **verificado** foi executado em 2026-11-04 (M2 e M2b do [plano de trabalho](plano-de-trabalho.md)) com `scripts/smoke.sh` ou com os roteiros descritos neste guia, sobre a imagem construída do `Dockerfile` do projeto. O que não foi executado está dito como tal. Para o modelo de ameaça e o que ainda não está coberto, veja [seguranca.md](seguranca.md).

## Índice

1. [Verificação rápida](#1-verificação-rápida)
2. [Subir o serviço](#2-subir-o-serviço)
3. [Variáveis de ambiente que importam](#3-variáveis-de-ambiente-que-importam)
4. [Chaves e tenants](#4-chaves-e-tenants)
5. [CLI](#5-cli)
6. [Ingestão por diretório](#6-ingestão-por-diretório)
7. [Backup do que importa](#7-backup-do-que-importa)
8. [Métricas e alertas](#8-métricas-e-alertas)
9. [Limites conhecidos](#9-limites-conhecidos)
10. [Problemas comuns](#10-problemas-comuns)

---

## 1. Verificação rápida

```bash
scripts/smoke.sh
```

Pré-requisitos: Docker, `curl`, `python3` (só biblioteca padrão), `awk` e `cmp`. Nada é instalado no host.

O script constrói a imagem (`syntra-engine:smoke`), sobe um contêiner **descartável** (SQLite, chave aleatória, volume próprio, porta alta ligada só em `127.0.0.1`), roda os passos abaixo e remove contêiner, volume e rede ao terminar, **inclusive em caso de falha ou Ctrl+C**. Cada passo imprime `PASS` ou `FAIL`; qualquer `FAIL` faz o script sair com código 1 e despejar o log do contêiner.

| Grupo | O que confere |
|---|---|
| Ciclo por arquivo (CSV de 5 MB, NDJSON, 2 MB aleatórios, arquivo vazio, 1 byte) | `compress` com os cabeçalhos `x-syntra-*`; `decompress` byte a byte (`cmp`); reenvio deduplicado; `GET /objects/:id`, `/content` e `/essence`; dado aleatório vira `stored` sem expandir |
| API | `analyze` (não grava), `codecs`, `health`, `ready`, `metrics` (`syntra_verify_failures_total 0`), OpenAPI, dashboard, `stats` |
| Erros e autenticação | sem chave ou chave errada: `401`; id malformado: `400`; id inexistente: `404`; nível inválido: `400`; container com 1 byte alterado: `422` |
| Ledger | `analyze`, `decompress` e leitura gravam `bytes_in > 0` e `cpu_ms > 0` (`/usage?events=500`) |
| Limites (contêiner extra com `SYNTRA_MAX_BODY_MB=1`) | `413` em JSON padrão (`payload_too_large`, mensagem e `hint`) em `/compress`, `/decompress` e `/analyze`; corpo pequeno ainda passa; envelope legítimo de 5 MB devolve `422 decompressed_too_large`; bombas de 1 GiB sem tamanho declarado (xz, bzip2) devolvem o mesmo `422` |
| Entrada hostil (contêiner principal) | bombas reais de 1 GiB em lz4, deflate, xz e bzip2 com tamanho declarado de 1 MiB: `422 decompressed_too_large` com `hint`, em menos de 10 s; envelope com varint colunar de 2^64-1: `422 restore_failed`; depois disso `/health` e `/content` seguem respondendo e o contêiner segue de pé |
| Publicação | `docker port` do contêiner de teste só em `127.0.0.1`; o endereço da máquina fora do loopback não responde; `docker compose config` dos dois arquivos mostra `127.0.0.1:3002` por padrão e aceita `SYNTRA_PUBLISH_HOST` e `SYNTRA_PUBLISH_PORT` |
| Tenants | criar tenant e chave, comprimir com a chave, `/usage` e cotação, `Idempotency-Key`, isolamento (`404`), escopos (`403`), revogação (`401`) |
| CLI | `scripts/syntra.sh` contra o próprio serviço (compress, decompress, get, analyze, inspect, list, stats, usage, codecs, health e os códigos de erro) |
| Operação | ingestão por diretório, remoção com e sem `purge`, reinício do contêiner (dados persistem e o arquivo inalterado do diretório observado **não** é reingerido: 1 referência, nenhum evento `compress` novo), encerramento por `SIGTERM` com código 0 |

Ao final, imprime a tabela `arquivo, tamanho, essência, economia %, plano, ok`.

| Variável | Padrão | Efeito |
|---|---|---|
| `SYNTRA_SMOKE_DB` | `sqlite` | `postgres` sobe também um Postgres descartável (`postgres:16-alpine`) numa rede própria |
| `SYNTRA_SMOKE_SKIP_BUILD` | `0` | `1` reaproveita a imagem já construída |
| `SYNTRA_SMOKE_IMAGE` | `syntra-engine:smoke` | tag da imagem |
| `SYNTRA_SMOKE_PG_IMAGE` | `postgres:16-alpine` | imagem do Postgres do modo `postgres` |
| `SYNTRA_SMOKE_READY_TIMEOUT` | `120` | segundos de espera por `/ready` |

Resultado de 2026-11-04 (fechamento do M2b), mesma máquina e imagem construída do `Dockerfile` do projeto: **SQLite, 100 PASS, 0 FAIL, 27 s**; **Postgres, 100 PASS, 0 FAIL, 31 s** (os dois com a imagem já em cache e sem sobras no Docker). No M2 eram 81 passos; o M2b acrescentou o ledger, os limites (`413` em JSON e teto de descompressão), as bombas e o envelope hostil, a publicação em loopback e a ingestão com reinício.

O modo `postgres` cobre o caminho que o `cargo test` **não** cobre (os testes usam `sqlite::memory:`).

Tempos medidos na máquina de desenvolvimento (32 threads, compartilhada) no M2: construção da imagem em **47 s** com a camada de dependências em cache (só o crate do motor recompila) e em **90 s e 93 s** em duas construções com `docker build --no-cache` (a segunda, de um instantâneo limpo do commit `a3b9ab0`); imagem final de 170 MB; serviço pronto em ~2 s. Com a imagem em cache o teste inteiro leva cerca de 30 s (27 s com SQLite e 31 s com Postgres no M2b; a geração das bombas de 1 GiB em Python soma uns 7 s). Em máquina menor, espere mais. O `HEALTHCHECK` da imagem (intervalo de 30 s) não foi observado até o estado `healthy`; o smoke consulta `/ready` diretamente.

---

## 2. Subir o serviço

### 2.1 Docker, sem compose (SQLite)

A imagem traz como padrão um Postgres em `postgres:5432`. Sem `SYNTRA_DB_URL` o contêiner **encerra com erro** (`conectando no PostgreSQL ... failed to lookup address information`; verificado). Para um nó único, SQLite basta:

```bash
docker build -t syntra-engine .
docker volume create syntra-data

docker run -d --name syntra --restart unless-stopped \
  -p 127.0.0.1:3002:3002 \
  -v syntra-data:/data \
  -e SYNTRA_DB_URL=sqlite:/data/syntra.db \
  -e SYNTRA_API_KEYS="$(python3 -c 'import secrets; print(secrets.token_hex(32))')" \
  syntra-engine
```

Guarde a chave. Os dados ficam no volume `syntra-data` (`/data` no contêiner). A imagem roda como usuário `syntra` (uid 1000) e tem `HEALTHCHECK` em `/ready`.

### 2.2 `docker compose` de desenvolvimento

```bash
mkdir -p watch_in
docker compose up --build
```

Sobe Postgres + motor, com esforço `max`, **sem chave** (API aberta) e dashboard em <http://localhost:3002/>. Verificado, num projeto compose separado (nome, portas e contêineres próprios): o ciclo comprimir/restaurar pelo CLI fechou byte a byte e `GET /api/v1/stats` respondeu `database.backend = postgres` e `auth_enabled = false`.

Dois cuidados (corrigidos no M2b; no M2 os dois eram defeitos, e o resultado de cada correção está no roteiro ao fim desta seção):

- **Crie `watch_in/` antes.** O diretório do host é montado em `/data/watch_in` com `create_host_path: false`: se ele não existir, o `up` **falha** (`bind source path does not exist`) em vez de o Docker criá-lo como `root:root`, que era o defeito (o usuário recebia `Permission denied` ao copiar arquivos). Criado por você, o diretório é seu. O motor roda como uid 1000: com outro uid no host, o motor lê os arquivos (diretório `755`), mas só consegue apagar a origem (`SYNTRA_WATCH_DELETE_SOURCE=true`) se o diretório for gravável por ele. `SYNTRA_WATCH_HOST_DIR` aponta outro diretório.
- **A porta é publicada só em `127.0.0.1`** por padrão (`SYNTRA_PUBLISH_HOST`, `SYNTRA_PUBLISH_PORT`); o Postgres também (`SYNTRA_POSTGRES_PORT`). Com a API aberta, não troque `SYNTRA_PUBLISH_HOST` para `0.0.0.0` sem pôr um proxy com chave na frente.

Para subir uma segunda instância (ou rodar ao lado de outros projetos), `SYNTRA_CONTAINER_PREFIX=outro docker compose -p outro up --build` troca os nomes dos contêineres, e as portas saem de `SYNTRA_PUBLISH_PORT` e `SYNTRA_POSTGRES_PORT`.

Roteiro verificado em 2026-11-04 (projeto compose próprio: `-p`, prefixo, portas e `SYNTRA_WATCH_HOST_DIR` fora do repositório; tudo removido com `down -v --rmi local`):

1. Sem o diretório: `docker compose up` falhou com `invalid mount config for type "bind": bind source path does not exist` e o diretório **não** foi criado.
2. Com o diretório criado por um usuário comum: Postgres e motor saudáveis; `docker port` mostrou `127.0.0.1:<porta>`; o endereço da máquina na LAN recusou a conexão; `GET /api/v1/stats` respondeu `backend = postgres`, `auth_enabled = false`.
3. Um arquivo copiado por esse usuário para `./watch_in` foi arquivado em 2 s, com conteúdo idêntico (`/content`), e o dono do diretório e do arquivo seguiu sendo o usuário (nenhum `root`).
4. Depois de `docker compose restart engine`: 1 objeto, 1 referência e 1 evento `compress` (nada reingerido, com Postgres); modificar o arquivo gerou o segundo objeto em 2 s.

### 2.3 `docker compose` de produção

```bash
export SYNTRA_API_KEYS="$(python3 -c 'import secrets; print(secrets.token_hex(32))')"
export POSTGRES_PASSWORD="$(python3 -c 'import secrets; print(secrets.token_hex(24))')"
docker compose -f docker-compose.prod.yml up -d --build
```

As duas variáveis são obrigatórias: sem elas `docker compose -f docker-compose.prod.yml config -q` falha com `defina SYNTRA_API_KEYS` e `defina POSTGRES_PASSWORD` (verificado; com as duas definidas a configuração valida). **O arquivo de produção foi levantado e verificado em 2026-11-04 (M2b)**, com `SYNTRA_API_KEYS`, `POSTGRES_PASSWORD`, `SYNTRA_CONTAINER_PREFIX` e `SYNTRA_PUBLISH_PORT` próprios (para não colidir com nada, e removido com `down -v --rmi local`): `up -d --build --wait` terminou com Postgres e motor `healthy`; a porta saiu em `127.0.0.1:<porta>` e o endereço da máquina na LAN recusou a conexão; sem chave `POST /api/v1/compress` devolveu `401`; com a chave `GET /api/v1/stats` mostrou `backend = postgres`, autenticação ativa e `max_body_mb` e `max_decompressed_mb` de 10240; `/ready` devolveu `ready: true`. Sem as duas variáveis, `docker compose -f docker-compose.prod.yml config` falha com `defina SYNTRA_API_KEYS` e `defina POSTGRES_PASSWORD`. Não foi observado: carga, retenção por semanas, `HEALTHCHECK` fora dos primeiros minutos, o comportamento atrás de proxy com TLS.

Pontos do arquivo: ingestão por diretório desligada (`SYNTRA_WATCH_ENABLED=false`), limite de corpo de **10240 MB** (e, sem `SYNTRA_MAX_DECOMPRESSED_MB`, teto de descompressão também de 10240 MB: veja a [seção 9](#9-limites-conhecidos) e [seguranca.md](seguranca.md#3-limites-configuráveis)), volumes nomeados `syntra_data_prod` e `syntra_postgres_prod_data` (com `SYNTRA_CONTAINER_PREFIX` o prefixo muda; sem ele os nomes são os de sempre) e a porta publicada em `127.0.0.1:3002` por padrão. Coloque um proxy com TLS na frente e restrinja `/metrics`, que é aberto por design.

### 2.4 Sem Docker

`SYNTRA_DB_URL=sqlite:syntra.db cargo run --release` (README). Não foi executado no M2: o ambiente de build do projeto é o contêiner (D2 do plano).

---

## 3. Variáveis de ambiente que importam

A tabela completa está no [README](../README.md#configuração). O que muda entre o binário, a imagem e os compose:

| Variável | Binário | Imagem | Compose dev | Compose prod |
|---|---|---|---|---|
| `SYNTRA_DB_URL` | `postgres://syntra:syntra123@localhost:5432/syntra_monitoring` | `postgres://syntra:syntra123@postgres:5432/syntra_monitoring` | Postgres do compose | Postgres do compose, senha de `POSTGRES_PASSWORD` |
| `SYNTRA_DEFAULT_EFFORT` | `balanced` | `balanced` | `max` | `balanced` |
| `SYNTRA_API_KEYS` | vazio (API aberta) | vazio | vazio | obrigatório |
| `SYNTRA_MAX_BODY_MB` | 4096 | 4096 | 4096 | 10240 |
| `SYNTRA_MAX_DECOMPRESSED_MB` | igual a `SYNTRA_MAX_BODY_MB` | idem | idem (repassa a variável do host, se houver) | idem |
| `SYNTRA_WATCH_ENABLED` | `true` | `true` | `true` | `false` |
| Caminhos | relativos ao diretório atual | `/data/...` | `/data/...` | `/data/...` |

Valor inválido em variável numérica, booleana ou de esforço gera aviso no log e cai no default; o serviço sobe mesmo assim. `SYNTRA_API_KEYS` vazio liga a API **aberta** no tenant `default` com todos os escopos e o banner de inicialização diz `ABERTA`.

---

## 4. Chaves e tenants

Com `SYNTRA_API_KEYS` definido, cada chave dessa lista é o **administrador de bootstrap**: opera no tenant `default` com todos os escopos. Com ela se cria o tenant e as chaves dos clientes (fluxo verificado pelo `smoke.sh`):

```bash
export SYNTRA_URL=http://127.0.0.1:3002
export ADMIN="<valor de SYNTRA_API_KEYS>"

curl -sS -X POST "$SYNTRA_URL/api/v1/admin/tenants" \
  -H "x-api-key: $ADMIN" -H 'content-type: application/json' \
  -d '{"id":"acme","name":"ACME S.A.","plan":"savings_share",
       "price":{"share_pct":30,"reference_gb_month_cents":12}}'

curl -sS -X POST "$SYNTRA_URL/api/v1/admin/tenants/acme/keys" \
  -H "x-api-key: $ADMIN" -H 'content-type: application/json' \
  -d '{"name":"producao","scopes":["compress","read"],"expires_in_days":365}'
```

A resposta traz `key` (formato `syn_<12 hex>_<64 hex>`) **uma única vez**; a listagem `GET /admin/tenants/:id/keys` devolve só metadados (verificado: o segredo não reaparece). Escopos: `compress`, `read`, `delete`, `admin`. Sem `scopes`, a chave nasce com `compress` e `read`.

Revogar tem efeito imediato (verificado: a chave revogada recebeu `401` na requisição seguinte):

```bash
curl -sS -X DELETE "$SYNTRA_URL/api/v1/admin/keys/<key_id>" -H "x-api-key: $ADMIN"
```

Consumo e cotação do tenant da chave usada: `scripts/syntra.sh usage` (ou `GET /api/v1/usage`). Para outro tenant, `GET /api/v1/admin/usage?tenant_id=acme`.

Duas propriedades verificadas que costumam surpreender:

- O mesmo conteúdo enviado por um segundo tenant **não** vem como `deduplicated: true`; a deduplicação entre tenants só aparece no ledger (`dedup_cross_tenant`).
- `Idempotency-Key` repetida no mesmo tenant devolve `replayed: true` e **não** gera novo evento de uso.

Tenants, chaves, grants e consumo vivem no **banco** (veja a [seção 7](#7-backup-do-que-importa)): perder o banco apaga as chaves dos clientes.

---

## 5. CLI

`scripts/syntra.sh` é um cliente fino sobre `curl`; só precisa de `curl` e `python3` (biblioteca padrão). Para usar de qualquer lugar: `ln -s "$PWD/scripts/syntra.sh" ~/.local/bin/syntra`.

```bash
export SYNTRA_URL=http://127.0.0.1:3002
export SYNTRA_API_KEY="<chave>"
```

`SYNTRA_URL` tem padrão `http://127.0.0.1:3002`. `SYNTRA_API_KEY` vai no cabeçalho `x-api-key` e **nunca é impressa**; o cabeçalho é entregue ao `curl` pela entrada padrão (`curl -K -`), não como argumento de linha de comando (o `smoke.sh` confere que a chave não surge em nenhuma saída do CLI).

| Comando | O que faz |
|---|---|
| `compress FILE [--effort fast\|balanced\|max] [--out FILE] [--json] [--force]` | Comprime e grava o container (padrão `FILE.syntra`). `--json` imprime o relatório do servidor e **não** grava o container (o objeto fica arquivado no servidor) |
| `decompress FILE.syntra [--out FILE] [--force]` | Reconstrói o original. Sem `--out`, escreve na saída padrão (recusa se for um terminal) |
| `get ID [--out FILE] [--force]` | Baixa o original de um objeto arquivado |
| `analyze FILE [--effort E] [--json]` | Compara os planos sem gravar nada |
| `inspect FILE.syntra [--json]` | Lê metadados e valida o checksum sem reconstruir |
| `list [--search TXT] [--limit N] [--json]` | Lista os objetos do tenant da chave |
| `stats [--json]` | Configuração, vault, banco e acumulados |
| `usage [--days N] [--events N] [--json]` | Consumo e cotação |
| `codecs [--json]` | Codecs, transforms e esforços |
| `health [--json]` | `/health` e `/ready`; sai com 1 se não estiver pronto |

Saída real de um `compress` (CSV sintético de 5,13 MB, esforço `balanced`):

```
$ scripts/syntra.sh compress vendas.csv --effort balanced
arquivo:      vendas.csv
id:           602304557f2216618a0fcb35f9549a99188e09c7157f8f50539eb43197038889
classe:       tabular
original:     5.13 MB (5128564 B)
essência:     185.23 KB (185226 B)
economia:     96.39%
plano:        csv_columnar+brotli:9
verificado:   true
deduplicado:  false
container:    vendas.csv.syntra (185.64 KB, 185637 B)
```

Comportamentos de segurança, todos verificados: não sobrescreve arquivo existente sem `--force` (código 2); recusa gravar binário no terminal; grava a saída num temporário e só move para o destino se a API respondeu 2xx, então um corpo de erro nunca vira "arquivo restaurado".

Erros da API saem legíveis (corpo `error.code`, `message`, `hint`):

```
$ scripts/syntra.sh get bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
erro 404 not_found: objeto bbbb...bbbb não encontrado
```

| Código de saída | Significa |
|---|---|
| 0 | sucesso |
| 1 | erro devolvido pela API (inclusive `401`, `404`, `413`, `422`) |
| 2 | uso incorreto (opção inválida, ID malformado, arquivo inexistente, sobrescrita recusada) |
| 3 | falha de conexão |

O CLI envia o arquivo com `curl -T` (envio contínuo, sem carregar o arquivo no cliente), mas o **servidor** lê o corpo inteiro em memória; veja a [seção 9](#9-limites-conhecidos).

---

## 6. Ingestão por diretório

Com `SYNTRA_WATCH_ENABLED=true` (padrão; **desligada** no compose de produção), o motor observa `SYNTRA_WATCH_DIR` (`/data/watch_in` nas imagens) e arquiva cada arquivo novo pela mesma camada de serviço da API. Comportamento lido em `src/watcher.rs` e confirmado pelo `smoke.sh` (um arquivo colocado no diretório virou objeto com bytes idênticos):

- **Não é recursivo**: subdiretórios são ignorados.
- Ignora arquivo **vazio**, nomes que começam com `.` ou `~` e extensões `.tmp`, `.part`, `.crdownload`, `.swp` e `.syntra`.
- Espera o tamanho **estabilizar** (3 leituras a cada 200 ms; desiste após 60 s) e varre o diretório a cada 2 s; no máximo 4 arquivos em paralelo.
- Arquiva no tenant **`default`**, com o esforço de `SYNTRA_DEFAULT_EFFORT` e `persist=true`.
- O arquivo é lido **inteiro em memória**, como na API.
- Não grava `.syntra` no diretório: a essência vai para o vault. Para recuperar o original, ache o objeto e baixe:

```bash
scripts/syntra.sh list --search relatorio.csv
scripts/syntra.sh get <ID> --out relatorio.csv
```

- Não apaga o original, a não ser com `SYNTRA_WATCH_DELETE_SOURCE=true` **e** arquivamento confirmado.
- **O registro do que já foi arquivado é persistido** (tabela `watch_seen` do banco: caminho, tamanho, mtime em ns, hash do objeto; [`src/watch_registry.rs`](../src/watch_registry.rs)) e carregado no boot. Corrigido no M2b: no M2 ele ficava só em memória, e depois de um reinício `references` foi de 1 para 2 e o ledger do tenant `default` ganhou um segundo evento `compress`. Agora:
  - arquivo inalterado (mesmo tamanho e mtime): ignorado, antes e depois de reiniciar;
  - só o mtime mudou: o motor relê o arquivo, confere o BLAKE3 contra o hash registrado e, sendo igual, só atualiza o registro (sem referência nem evento novo);
  - conteúdo diferente: vira objeto novo; o objeto da versão anterior **mantém** a sua referência (solte com `DELETE /objects/:id`);
  - arquivo removido do diretório: sai do registro;
  - registro ilegível ou banco novo: tudo é tratado como novo (o log avisa), então um banco perdido reingere o diretório.
  Não percebe um arquivo trocado por outro de mesmo tamanho **e** mesmo mtime. Verificado pelo smoke: 1 referência e nenhum evento `compress` novo depois de `docker restart`.

Em Docker com volume nomeado, entregue o arquivo pelo próprio contêiner (o usuário `syntra` precisa ser o dono):

```bash
docker exec -i syntra sh -c 'cat > /data/watch_in/relatorio.csv' < relatorio.csv
```

No compose de desenvolvimento o diretório é o bind mount `./watch_in` (veja o cuidado da [seção 2.2](#22-docker-compose-de-desenvolvimento)).

---

## 7. Backup do que importa

| O quê | Onde | Se perder |
|---|---|---|
| **Essências** | `SYNTRA_VAULT_PATH` (`/data/essence_vault`), um `.syntra` por conteúdo único em `<2 hex>/<2 hex>/<id>.syntra` | O conteúdo se perde; não há outra cópia |
| **Banco** | SQLite em `/data/*.db` ou o Postgres | Perdem-se tenants, chaves, grants e o histórico de consumo que sustenta a fatura |
| **Dicionários** | `SYNTRA_DICT_PATH` (`/data/dictionaries`) | Essência comprimida com dicionário devolve `409 dictionary_unavailable` até restaurá-lo (README; não exercitado no smoke, que não treina dicionários) |
| Índice sled | `SYNTRA_SLED_INDEX` (`/data/metadata_index.sled`) | Nada: é reconstruído a partir do vault no boot |

Faça backup dos três primeiros **juntos**.

**Volume do Docker** (pare o motor antes, para o arquivo SQLite e o vault ficarem consistentes; verificado, em volume de teste):

```bash
docker stop syntra
docker run --rm -v syntra-data:/data:ro -v "$PWD":/backup \
  --entrypoint tar syntra-engine czf /backup/syntra-data.tgz -C /data .
docker start syntra
```

**Postgres**: use `pg_dump` (não executado no M2).

O que foi verificado sobre perdas (volume de teste, SQLite):

1. **Perder o índice sled** (`rm -rf /data/metadata_index.sled` e reiniciar): o motor registrou `índice do vault reconciliado essencias_no_vault=1 reindexadas=1`, e `/objects/:id/content` continuou respondendo `200`.
2. **Perder o banco** (`rm /data/*.db*` e reiniciar): `GET /objects` passou a listar 0, `/objects/:id/content` devolveu `404` (sem grant), mas `POST /decompress` com o `.syntra` guardado reconstruiu o arquivo byte a byte; reenviar o original recriou o grant (`deduplicated: false`, porque o tenant já não tinha grant) e o `/content` voltou a `200`. As chaves e os tenants **não** voltam sozinhos.

Um `.syntra` é auto-suficiente (nome, hash, plano, receita de reconstrução): guardar os containers que o `compress` devolveu é um backup independente do servidor, desde que o dicionário usado (se houver) também seja guardado.

---

## 8. Métricas e alertas

`GET /metrics` (Prometheus; **aberto**, não exige chave) devolve, entre outras, as linhas abaixo (todas observadas no smoke):

```
syntra_bytes_in_total, syntra_essence_bytes_total, syntra_distillation_ratio
syntra_items_total, syntra_reconstructions_total, syntra_errors_total
syntra_verify_failures_total
syntra_dedup_hits_total, syntra_dedup_bytes_saved_total
syntra_codec_usage_total{codec="..."}
syntra_processing_latency_ms_bucket{le="..."}, syntra_processing_latency_ms_sum, syntra_processing_latency_ms_count
syntra_uptime_seconds, syntra_system_cpu_usage, syntra_system_memory_used_bytes
```

`syntra_items_total` conta **toda** chamada de `compress`, inclusive as deduplicadas; `syntra_dedup_hits_total` conta as reaproveitadas.

Alertas mínimos:

```yaml
groups:
  - name: syntra
    rules:
      - alert: SyntraVerifyFailure
        expr: syntra_verify_failures_total > 0
        annotations:
          summary: uma essência não reproduziu o original na verificação de gravação
      - alert: SyntraErrors
        expr: increase(syntra_errors_total[5m]) > 0
      - alert: SyntraNotReady
        expr: probe_success{job="syntra-ready"} == 0
```

A terceira regra pressupõe um *blackbox exporter* sondando `/ready`; o motor não exporta esse sinal.

`/ready` devolve `200` com `{"ready":true,"checks":{"database":true,"vault":true}}` e `503` quando o banco ou o vault falham; é o que o `HEALTHCHECK` da imagem usa. `/health` não toca banco nem disco.

---

## 9. Limites conhecidos

Verificados no código e, onde há número, medidos em 2026-11-04.

### Arquivo inteiro em memória

O corpo HTTP é lido por completo (`Bytes`, [`src/api/compress.rs:25`](../src/api/compress.rs)), copiado para um `Vec`, comprimido, **descomprimido de novo** para a verificação e só então gravado. Restaurar e baixar (`/content`, `/essence`, `/decompress`) também carregam tudo. A ingestão por diretório usa `tokio::fs::read`. Não há envio contínuo (*streaming*) em nenhuma rota.

Medição única, sem repetição, de um CSV sintético de **302,4 MB** (esforço `balanced`, container Docker sem limite de memória, `VmHWM` de `/proc/1/status`; linha de base do processo ocioso: 73 MB; MB = 10^6 bytes):

| Operação | Pico de memória (RSS) | Múltiplo do arquivo |
|---|---:|---:|
| `POST /compress` (com verificação, plano `zstd:12`, 94,74% de economia, 7,8 s) | 1.054 MB | **3,5x** |
| `GET /objects/:id/content` | 734 MB | 2,4x |
| `POST /decompress` (container de 15,9 MB) | 764 MB | 2,5x |
| `POST /analyze` | 831 MB | 2,7x |

Isso confirma a estimativa do [ADR 001](adr-001-envelope-v4-blocos.md) (≥3x o arquivo, que era inferência) para a compressão: 3,5x nesta medição. **Dimensione a RAM por `3,5 x maior arquivo x requisições simultâneas`**: o corpo é lido *fora* do semáforo de CPU, então uploads simultâneos somam. Coloque um limite de memória no contêiner (`--memory`) e um limite de corpo coerente. Roteiro para repetir a medição:

```bash
docker exec syntra grep -E 'VmHWM|VmRSS' /proc/1/status
```

(O pico `VmHWM` só cresce; reinicie o contêiner entre operações para medir cada uma.)

### Limite de corpo

`SYNTRA_MAX_BODY_MB` vale **4096** por padrão e **10240 no compose de produção**; com 3,5x, um único arquivo no limite do compose de produção pede da ordem de 36 GB de RAM, sem contar o paralelismo.

Acima do limite o motor devolve `413` **em JSON**, no mesmo formato do resto da API (`error.code = payload_too_large`, `message` com o limite em MB e `hint` citando `SYNTRA_MAX_BODY_MB`), em todas as rotas com corpo, com `x-request-id` e CORS (corrigido no M2b; no M2 a resposta era texto puro em inglês). Verificado pelo smoke com `SYNTRA_MAX_BODY_MB=1` e um envio de 2 MiB, e por um teste que percorre as 11 rotas com corpo. O contrato OpenAPI documenta o `413` nas rotas com corpo.

### Teto de descompressão

`SYNTRA_MAX_DECOMPRESSED_MB` (padrão: o valor de `SYNTRA_MAX_BODY_MB`) limita a saída de **toda** reconstrução, em todos os codecs. O limite efetivo é o menor entre ele e o `original_size` do envelope; estouro devolve `422 decompressed_too_large` com `hint`. Protege contra envelopes forjados e bombas de descompressão (1 GiB de zeros cabe em poucos KB), mas o padrão é alto: veja os riscos que restam e como escolher o valor em [seguranca.md](seguranca.md#3-limites-configuráveis). Atenção: `/compress` não confere o original contra este teto; com o teto abaixo do limite de corpo, um objeto grande é arquivado e depois `/content` devolve `422` (`/essence` ainda entrega o container).

### Envelope único

Cada objeto é **uma** mensagem protobuf (`ProcessedFile`) com todo o payload no campo `data`, protegida por um checksum de 64 bits do payload inteiro ([ADR 001, seção 1](adr-001-envelope-v4-blocos.md)). Consequências:

- um bit alterado invalida o objeto inteiro e o erro **não localiza** o dano (verificado: container com 1 byte alterado devolve `422 envelope_corrupted`, sem devolver dado);
- não há leitura por faixa (`Range`): restaurar 1 KB de um objeto de 1 GB reconstrói o objeto;
- payloads entre 2 e 4 GiB num único campo protobuf **não foram verificados** (risco anotado no ADR); não os use sem testar;
- acima de 48 MB (`SYNTRA_PROBE_THRESHOLD_MB`) o plano é escolhido por uma amostra de 8 MB e aplicado ao arquivo todo.

O formato v4 em blocos (ADR 001) está em desenho e implementação isolada (marco M1) e **não** está ligado à API.

### Outros

- **Tamanho mínimo do container.** O container acrescenta metadados ao payload: nas medições, +200 B (arquivo vazio), +339 B (1 byte), +367 B (2 MB aleatórios), +411 B (CSV), +429 B (NDJSON). Para arquivos de poucos bytes o `.syntra` é **maior** que o original, embora o payload nunca seja.
- **Nome do objeto.** O nome registrado é o do **primeiro** envio do tenant para aquele conteúdo; envios posteriores do mesmo conteúdo com outro nome só incrementam `references`. `GET /objects?search=` é uma busca por trecho **desse nome**; não existe o parâmetro `type` (`name|mime|id`) que o README descrevia antes do M2. O `id` é o hash BLAKE3 e é a chave estável.
- **Ledger.** Corrigido no M2b: `analyze`, `decompress` e leitura gravam `bytes_in`, `bytes_out` e `cpu_ms` (no M2 saíam `bytes_in = 0` e `cpu_ms = 0`). `totals.bytes_in` do `/usage` soma todas as operações; use `by_operation` para separar, e note que a cotação e o `savings_pct` usam só `compress`.
- **Sem TLS e CORS aberto.** O motor fala HTTP puro e, sem `SYNTRA_CORS_ORIGINS`, libera qualquer origem. Use proxy reverso.
- **Dashboard com autenticação.** `GET /` abre sem chave, mas as rotas de dados que ele consome (`/stats`, `/api/files`) exigem chave: com autenticação ativa a página fica sem dados (verificado: `/stats` sem chave devolve `401`).
- **Ingestão por diretório:** segue links simbólicos e não percebe arquivo trocado por outro de mesmo tamanho e mtime; veja a [seção 6](#6-ingestão-por-diretório) e [seguranca.md](seguranca.md#6-o-que-ainda-não-está-coberto).
- **Segurança.** Sem TLS, sem limite de taxa por IP ou chave, sem limite de concorrência de upload e sem timeout de leitura do corpo no motor: tudo isso fica no proxy. Lista completa em [seguranca.md](seguranca.md#6-o-que-ainda-não-está-coberto).
- **Desempenho de servidor não medido.** Os números de vazão do README e do [benchmark](../bench/README.md) são de um notebook híbrido; **não existe medição do serviço sob carga**, nem em servidor, nem com I/O real de rede e disco.

---

## 10. Problemas comuns

| Sintoma | Causa provável | O que fazer |
|---|---|---|
| Contêiner sai ao iniciar com `conectando no PostgreSQL` | `SYNTRA_DB_URL` ausente: a imagem aponta para `postgres:5432` | Defina `SYNTRA_DB_URL=sqlite:/data/syntra.db` ou use o compose |
| `401 unauthorized` | Chave ausente ou errada | Envie `x-api-key: <chave>` ou `Authorization: Bearer <chave>` |
| `403 forbidden` | A chave não tem o escopo da rota | Emita outra chave com o escopo (`compress`, `read`, `delete`, `admin`) |
| `404 not_found` num id que você acabou de enviar | O id pertence a **outro tenant**, ou o banco foi perdido (sem grant) | Confira a chave usada; veja a [seção 7](#7-backup-do-que-importa) |
| `400 invalid_object_id` | O id não tem 64 dígitos hexadecimais | Use o `x-syntra-id` devolvido pelo `compress` |
| `409 dictionary_unavailable` | Falta o dicionário com que a essência foi comprimida | Restaure `SYNTRA_DICT_PATH` |
| `413 payload_too_large` | Corpo acima de `SYNTRA_MAX_BODY_MB` | Aumente o limite (e a RAM) ou divida o arquivo |
| `422 decompressed_too_large` | A reconstrução passaria do teto (o menor entre o `original_size` do envelope e `SYNTRA_MAX_DECOMPRESSED_MB`): objeto grande demais para o teto, ou envelope forjado | Se o objeto é legítimo, aumente `SYNTRA_MAX_DECOMPRESSED_MB`; senão, recuse o envelope |
| `422 envelope_corrupted` | O container mudou depois de gerado | Use outra cópia do `.syntra` ou recomprima o original |
| `docker compose up` falha com `bind source path does not exist` | `./watch_in` não existe (o compose não o cria, de propósito) | `mkdir -p watch_in` e suba de novo |
| `Permission denied` ao copiar para `watch_in/` | O diretório foi criado como root (compose do M2 ou outro meio) | Com o contêiner parado e o diretório vazio, `rmdir watch_in && mkdir watch_in`; suba de novo |
| Arquivo colocado em `watch_in` não é arquivado | Vazio, oculto, `.tmp`/`.part`, subdiretório, ou ingestão desligada | Veja a [seção 6](#6-ingestão-por-diretório) |
| Pico de memória muito acima do tamanho do arquivo | Arquivo inteiro em RAM, com cópia de verificação | Veja a [seção 9](#9-limites-conhecidos) |
