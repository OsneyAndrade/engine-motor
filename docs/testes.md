# Testes do motor

Convenção do projeto (D2 do [plano de trabalho](plano-de-trabalho.md)): **build e testes só em contêiner Docker**, código montado somente leitura, nada instalado no host. Comandos validados em 2026-11-04 com `rust:1.88-slim-bookworm`.

## 1. Suíte completa

```
docker run --rm --cpuset-cpus=16-31 \
  -v "$PWD":/src:ro -v syntra-cargo:/usr/local/cargo/registry -v syntra-target:/target \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.88-slim-bookworm cargo test --locked
```

- `--cpuset-cpus=16-31` prende o build aos E-cores deste notebook para não perturbar medições nos P-cores; ajuste ou remova em outra máquina.
- `syntra-target` é o volume de artefatos. Quem rodar em paralelo com outra sessão deve usar **outro volume** (`syntra-target-m1`, por exemplo) para não disputar o *lock* do `target`.
- Resultado esperado no marco M1: **174 passed, 0 failed, 2 ignored** (122 testes anteriores + 3 do corpus dourado + 41 do formato v4 + 8 do *probe*; os 2 ignorados são o gerador do corpus e o microbenchmark).

Subconjuntos (acrescente ao final do comando, depois de `cargo test --locked`):

| Filtro | O que roda |
|---|---|
| `golden` | restauro do corpus dourado v2/v3 e seus limites |
| `blockfile` | formato v4: round-trip, fuzz do parser, corrupção injetada, compatibilidade |
| `probe` | *probe* de incompressibilidade |
| `blockfile::tests::fuzz -- --nocapture` | imprime quantas entradas o fuzz do parser gerou (24 977 no marco M1) |

O clippy não vem na imagem. Instale-o **dentro do contêiner efêmero** (nada fica no host):

```
docker run --rm --cpuset-cpus=16-31 \
  -v "$PWD":/src:ro -v syntra-cargo:/usr/local/cargo/registry -v syntra-target:/target \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.88-slim-bookworm \
  sh -c 'rustup component add clippy && cargo clippy --all-targets --locked'
```

Em 2026-11-04 os módulos novos (`blockfile`, `blockfile_tests`, `probe`, `golden`) saem sem avisos; restam avisos antigos em `codec`, `content`, `dict_store`, `planner`, `tenancy`, `transform`, `usage` e `vault`, que o M1 não tocou.

## 2. Corpus dourado v2/v3

Local: `src/testdata/golden/` (1,33 MB, 73 arquivos). Cada original tem no máximo 64 KiB e o total fica abaixo de 2 MiB (verificado por teste).

```
manifest.json              lista cada caso: nome, original, envelope, versão, plano, tamanho,
                           BLAKE3 do original, tamanho e BLAKE3 do envelope, dicionário usado
originais/                 10 arquivos: texto, CSV, NDJSON, f32, i64, pseudo-executável,
                           aleatório, corridas, vazio, 1 byte
envelopes/                 61 envelopes (57 v3 e 4 v2), extensão .envelope
dicionarios/               dicionário zstd treinado (.zdict) resolvido pelo teste
```

Cobertura: codecs `stored`, `lz4`, `deflate`, `zstd`, `bzip2`, `brotli` e `xz`; transforms `delta` (larguras 1, 2, 4, 8), `byte_split`, `rle` e `csv_columnar` (também encadeados); planos escolhidos pelo planejador (`compress_best`, esforços `fast`, `balanced` e `max`); dicionário zstd por `DictionaryRef` (v3) e por `dictionary_id` legado (v2); envelopes v2 com e sem `envelope_checksum`; entrada vazia e de 1 byte.

O teste `golden::restaura_todo_o_corpus` percorre o manifesto e, para cada caso, confere o BLAKE3 e o tamanho do original **e** do envelope (detecta fixture adulterada), chama `container::parse` + `container::restore` (o caminho de produção), compara os bytes e o BLAKE3 restaurados, e exige que o corpus cubra todos os codecs, transforms e versões acima.

### Regenerar

A geração é o teste `golden::gera_corpus` (`#[ignore]`), que escreve no diretório da variável `SYNTRA_GOLDEN_OUT`. Monte um diretório gravável no contêiner:

```
mkdir -p /tmp/golden-out && chmod 777 /tmp/golden-out
docker run --rm --cpuset-cpus=16-31 \
  -v "$PWD":/src:ro -v syntra-cargo:/usr/local/cargo/registry -v syntra-target:/target \
  -v /tmp/golden-out:/out -e SYNTRA_GOLDEN_OUT=/out \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.88-slim-bookworm \
  cargo test --locked golden::gera_corpus -- --ignored --nocapture
cp -r /tmp/golden-out/. src/testdata/golden/
docker run --rm -v /tmp/golden-out:/out rust:1.88-slim-bookworm sh -c 'rm -rf /out/*'
```

Os arquivos gerados no contêiner pertencem ao root; a cópia (`cp`) já fica do seu usuário, e a última linha apaga o resto sem `sudo`.

**Quando regenerar e quando não.** O valor do corpus está em ser **gerado pelo código antigo**: ele prova que envelopes gravados antes de uma mudança continuam restauráveis. Regenerar com `container.rs`, `planner.rs` ou `codec.rs` alterados troca o corpus por envelopes novos e perde essa prova. Por isso:

- Para **acrescentar casos**, gere num diretório à parte e copie só os arquivos novos, **mais** as linhas novas do `manifest.json` (edite o manifesto à mão); não sobrescreva os envelopes existentes (cada geração grava `created_at` novo, então os BLAKE3 dos envelopes mudam).
- Para regenerar tudo, faça-o apenas a partir de um *commit* cujo código de envelope v≤3 seja o mesmo do corpus que está sendo substituído, e confira que `git diff` mostra só mudança de timestamps nos envelopes.
- O manifesto e as fixtures nunca devem ser editados para fazer um teste passar: se `restaura_todo_o_corpus` falhar, o código quebrou a compatibilidade.

## 3. Fuzz, corrupção injetada e limite de memória

Os testes do formato v4 usam um PRNG xorshift determinístico (sem dependências novas) e rodam sempre com a suíte:

- **A3, round-trip:** vazio, 1 byte, tamanhos não múltiplos do bloco, exatamente N blocos, 1000 blocos de 4 KiB, todos os codecs e níveis extremos, `delta`/`byte_split` em todas as larguras, `rle`, `csv_columnar` (blocos com linhas inteiras), plano misto por bloco, blocos `stored`, dicionário, leitura em fluxo (`Read + Seek`) sobre arquivo de verdade, e 300 casos aleatórios de propriedade.
- **A4, fuzz do parser:** 8000 entradas aleatórias, 5000 rodapés forjados **com hash válido** (exercitam a validação, não só o hash), cada byte do cabeçalho, do rodapé e do final alterado com 9 máscaras e truncamento em todas as posições, mais extensões com lixo. Regra: `Err` tipado (com código conhecido) ou restauro idêntico; nunca pânico.
- **A5, corrupção:** 10 000 *bit flips* aleatórios no payload de um arquivo de 64 blocos; cada um é detectado como `block_corrupted` **antes** de decodificar, com o índice certo, e os outros blocos continuam legíveis. Também: blocos trocados de lugar, rodapé e final truncados, buraco no meio dos payloads.
- **A7, *probe*:** aleatório e dado já comprimido (zstd, xz, brotli, deflate, bzip2) são incompressíveis; texto, CSV, zeros e floats suaves não.

Para conferir que o parser não depende de memória grande, rode os testes hostis com teto de memória (o binário vem de um `cargo test --no-run` anterior; o nome com hash aparece na saída dele):

```
docker run --rm --memory=200m --memory-swap=200m -v "$PWD":/src:ro -v syntra-target:/target -w /src \
  rust:1.88-slim-bookworm /target/debug/deps/engine-<hash> --test-threads=1 \
  blockfile::tests::fuzz blockfile::tests::rodapes_hostis blockfile::tests::campos_do_cabecalho \
  blockfile::tests::tamanho_declarado blockfile::tests::payload_lixo blockfile::tests::dez_mil \
  blockfile::tests::truncar
```

Em 2026-11-04 esses sete testes passaram sob 200 MiB.

## 4. Microbenchmarks (`#[ignore]`)

O microbenchmark do *probe* precisa de build otimizado. O perfil `release` do projeto usa LTO total e demora; para medir, desligue o LTO só na linha de comando (o `Cargo.toml` não muda):

```
docker run --rm --cpuset-cpus=16-31 \
  -v "$PWD":/src:ro -v syntra-cargo:/usr/local/cargo/registry -v syntra-target:/target \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.88-slim-bookworm \
  cargo test --release --locked --config profile.release.lto=false \
  --config profile.release.codegen-units=16 --no-run
```

Rode o binário impresso (`/target/release/deps/engine-<hash>`) **num único núcleo**, fora do cargo, para reduzir o ruído:

```
docker run --rm --cpuset-cpus=20 -v "$PWD":/src:ro -v syntra-target:/target -w /src \
  rust:1.88-slim-bookworm /target/release/deps/engine-<hash> \
  --ignored microbenchmark_do_probe --nocapture
```

Saída: µs por bloco de 1 MiB do *probe* (aleatório, texto, CSV), só da coleta da amostra, de `zstd-1` e `zstd-3` (via `codec::encode`) sobre o mesmo bloco, e a razão entre eles.

Medição de 2026-11-04 (um E-core, release sem LTO, três execuções, variação abaixo de 1%):

| Item | µs por bloco de 1 MiB |
|---|---|
| *probe* em bloco aleatório | 3,66 a 3,67 |
| *probe* em texto / CSV | 12,1 / 10,4 |
| coleta da amostra (4 x 1 KiB) | 0,07 |
| `zstd-1` em aleatório | 323 a 326 |
| `zstd-3` em aleatório | 359 a 362 (2,9 GB/s neste núcleo) |
| `zstd-3` em texto | 2620 a 2650 |

Razão *probe* / `zstd-3` em bloco incompressível: **1,01 a 1,02%** (0,14% em texto). Ressalva: este E-core detecta incompressível a 2,9 GB/s, não aos ~5,5 GB/s do benchmark de referência; o custo do *probe* é praticamente constante (amostra de 4 KiB), então num núcleo em que o `zstd-3` leve ~190 µs o mesmo *probe* seria ~2%, no limite da meta. Medir em P-core ou no servidor-alvo quando houver (T1.5, T1.11).
