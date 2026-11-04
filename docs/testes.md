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
- Resultado esperado no fechamento do M2b: **224 passed, 0 failed, 2 ignored** (em tempo de parede de cerca de 70 s). No marco M1 eram 174 (122 testes anteriores + 3 do corpus dourado + 41 do formato v4 + 8 do *probe*); o M2b somou a suíte de robustez, as bombas, os testes de `413`, de ledger, de ingestão e de teto de descompressão. Os 2 ignorados são o gerador do corpus e o microbenchmark.

Subconjuntos (acrescente ao final do comando, depois de `cargo test --locked`):

| Filtro | O que roda |
|---|---|
| `golden` | restauro do corpus dourado v2/v3 e seus limites |
| `blockfile` | formato v4: round-trip, fuzz do parser, corrupção injetada, compatibilidade |
| `probe` | *probe* de incompressibilidade |
| `robustez` | fuzz determinístico das inversões de transform, dos decoders de todos os codecs, de envelopes forjados e da mutação do corpus dourado (Seção 5); `-- --nocapture` imprime as linhas `FUZZ ...` com os contadores |
| `bombas` | bombas reais de 1 GiB de zeros em lz4, zstd, brotli, xz, bzip2 e deflate, contra `codec::decode_limited`, `container::restore_with_limit` e o formato v4 (a primeira chamada gera as seis bombas, cerca de 20 s) |
| `watcher` | ingestão por diretório, incluindo o registro persistido e o reinício |
| `api::e2e` | API ponta a ponta em processo: `413` e `422` em JSON, ledger, bombas pela rota `/decompress`, cobrança |
| `blockfile::tests::fuzz -- --nocapture` | imprime quantas entradas o fuzz do parser gerou (24 977 no marco M1) |

O clippy não vem na imagem. Instale-o **dentro do contêiner efêmero** (nada fica no host):

```
docker run --rm --cpuset-cpus=16-31 \
  -v "$PWD":/src:ro -v syntra-cargo:/usr/local/cargo/registry -v syntra-target:/target \
  -e CARGO_TARGET_DIR=/target -w /src rust:1.88-slim-bookworm \
  sh -c 'rustup component add clippy && cargo clippy --all-targets --locked'
```

Em 2026-11-04, no fechamento do M2b, `cargo clippy --all-targets --locked` termina **sem nenhum aviso** (os 16 avisos antigos de `codec`, `content`, `dict_store`, `planner`, `tenancy`, `transform`, `usage` e `vault` foram corrigidos sem mudar comportamento; funções que só os testes usam, como `codec::decode`, `container::restore`, `transform::invert` e `transform::invert_chain`, passaram a `#[cfg(test)]`, o que também impede o código de produção de chamar um decodificador sem teto).

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

## 5. Robustez diante de entrada hostil (M2b)

Todos os testes abaixo usam PRNG xorshift com sementes fixas (reproduzíveis), rodam com a suíte e não têm dependências novas. Cada um captura pânico (`catch_unwind`), mede o pico de alocação do Rust (`src/memtrack.rs`, alocador contador só em teste), cronometra cada caso (limite de 10 s) e tem um vigia que aborta o processo se um caso passar de 60 s.

| Teste (`src/robustez_tests.rs`) | Casos | O que gera |
|---|---:|---|
| `fuzz_delta_...`, `fuzz_byte_split_...`, `fuzz_rle_...` | 25.000 cada | entradas aleatórias, amostras válidas mutadas e pares de controle RLE hostis, com larguras e `input_len` hostis, contra `invert` e `invert_limited` com teto aleatório |
| `fuzz_csv_colunar_comprimentos_hostis_...` | 25.000 | cabeçalho colunar válido com varints de comprimento hostis (2^64-1, `usize::MAX`, 10 bytes `0xFF`) |
| `fuzz_csv_colunar_dimensoes_hostis_...` | 25.000 | linhas e colunas hostis (0, 2^63, `usize::MAX`, produtos que estouram) |
| `fuzz_csv_colunar_mutacoes_do_formato_valido` | 25.000 | tabela colunar real, mutada (bit flip, truncamento, inserção, remoção, varint sujo) |
| `fuzz_dos_decoders_de_todos_os_codecs` | 3.000 por codec (21.000) | payload real mutado ou aleatório, `expected_len` ausente, verdadeiro ou hostil, teto de 1 MiB |
| `fuzz_envelopes_forjados_com_checksum_valido` e `..._com_tamanho_original_hostil` | 20.000 cada | `ProcessedFile` forjado e selado (checksum válido) com codec, versão, transforms, dicionário e `file_hash` hostis |
| `fuzz_de_mutacao_do_corpus_dourado_nao_causa_panico` | 24.400 (400 por envelope) | os 61 envelopes v2/v3 do corpus dourado com mutação nos bytes serializados, na camada de transform e nos campos |

Regras de aceite de cada caso: nenhum pânico, nenhuma saída acima do teto, pico de alocação proporcional à entrada, tempo < 10 s. O contador `FUZZ ...` de cada teste imprime `casos`, `panicos`, `saidas_acima_do_teto`, `estouros_de_memoria`, `ok`, `erros`, `pico_max_bytes` e `tempo_max_ms`.

`src/bombas_tests.rs` gera bombas **reais** de 1 GiB de zeros (zstd com tamanho declarado e sem, brotli, xz, bzip2, deflate e um bloco lz4 construído à mão) e exige: recusa em menos de 10 s, erro tipado com o teto certo, pico de alocação até o teto mais folga de leitura, capacidade final do buffer sem passar do teto, e o limite exato (saída igual ao teto passa, um byte acima falha) em todos os codecs. Também cobre `stored`, prefixo LZ4 inflado, `original_size` forjado, amplificação por RLE encadeado, cadeia de 9 transforms e a recusa no formato v4. A rota `/decompress` recebe as mesmas bombas no teste `bombas_reais_de_1_gib_em_todos_os_codecs_viram_422_rapido_pela_api`.

### Como reproduzir "o teste falha no código antigo"

O M2b exige que cada defeito tenha teste que falha antes e passa depois. Procedimento usado (nada disso entra no repositório):

```
mkdir /tmp/antigo && git archive a3b9ab0 -- Cargo.toml Cargo.lock build.rs proto src static | tar -x -C /tmp/antigo
```

Copie para `/tmp/antigo/src` os arquivos `robustez_tests.rs`, `memtrack.rs`, `bombas_tests.rs` (sem as linhas do `blockfile`) e `testdata/golden/`, registre os módulos em `main.rs` (alocador contador sob `#[cfg(test)]`) e acrescente **só** estas funções de cola, que repassam para as funções antigas sem teto: `codec::decode_limited`, `transform::invert_limited`, `container::restore_with_limit`, `codec::too_large_limit` (devolve `None`) e a variante `ContainerError::DecompressedTooLarge`. Rode cada teste isolado (`cargo test --locked <nome> -- --exact`) no mesmo contêiner, com `--memory=12g`, porque no código antigo alguns casos abortam o processo. O resultado de 2026-11-04 está em [seguranca.md](seguranca.md#5-evidência-medida-2026-11-04).
