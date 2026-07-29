//! # Módulo de Gerenciamento do Vault (Cofre)
//!
//! Este módulo gerencia o armazenamento de arquivos processados (essências).
//! O vault é onde os arquivos comprimidos são salvos no disco.
//!
//! # Estrutura do Vault
//!
//! Os arquivos são organizados em uma estrutura fragmentada para performance:
//!
//! ```text
//! essence_vault/
//!   ab/
//!     cd/
//!       abcd...1234.syntra
//!   ef/
//!     gh/
//!       efgh...5678.syntra
//! ```
//!
//! Esta estrutura é usada porque:
//! - Evita milhares de arquivos em um único diretório
//! - Permite acesso rápido pelo hash
//! - Funciona bem em sistemas de arquivos diferentes
//!
//! # O que é Sled?
//!
//! Sled é um banco de dados chave-valor embutido em Rust. É usado aqui
//! para criar um índice rápido que permite encontrar arquivos pelo hash.

use std::path::{Path, PathBuf};
use std::fs;
use anyhow::Result;
use sled::Db;
use crate::config::ESSENCE_EXTENSION;

/// Gerenciador do cofre de arquivos processados.
///
/// Esta estrutura gerencia tanto o armazenamento em disco quanto o índice
/// no banco de dados Sled para busca rápida.
///
/// # Campos
///
/// * `base_dir` - Diretório base onde os arquivos são armazenados
/// * `index` - Banco de dados Sled para indexação rápida
pub struct VaultManager {
    /// Diretório base onde os arquivos processados são salvos
    base_dir: PathBuf,

    /// Índice Sled (banco de dados chave-valor embutido)
    ///
    /// A chave é o hash do arquivo (32 bytes) e o valor é o nome do arquivo.
    /// Isso permite encontrar rapidamente se um hash já existe no vault.
    index: Db,
}

impl VaultManager {
    /// Cria um novo gerenciador de vault.
    ///
    /// # O que faz esta função?
    ///
    /// 1. Cria o diretório base se não existir
    /// 2. Abre o banco de dados Sled para indexação
    /// 3. Retorna uma instância pronta para uso
    ///
    /// # Parâmetros
    ///
    /// * `base_dir` - Diretório onde os arquivos serão armazenados
    /// * `index_path` - Caminho para o arquivo de índice do Sled
    ///
    /// # Retorna
    ///
    /// `Result<Self>` - Uma nova instância ou erro se falhar
    ///
    /// # Erros
    ///
    /// Retorna erro se:
    /// - Não conseguir criar o diretório base
    /// - Não conseguir abrir o banco Sled
    pub fn new(base_dir: impl AsRef<Path>, index_path: impl AsRef<Path>) -> Result<Self> {
        // Converte o parâmetro para PathBuf
        //
        // AsRef<Path> é uma trait que permite aceitar diferentes tipos:
        // - String
        // - &str
        // - PathBuf
        // - &Path
        // to_path_buf() converte para PathBuf.
        let base_dir = base_dir.as_ref().to_path_buf();

        // Cria o diretório base se não existir
        //
        // create_dir_all cria o diretório e todos os diretórios pais.
        // Se já existir, não faz nada e retorna Ok(()).
        if !base_dir.exists() {
            fs::create_dir_all(&base_dir)?;
        }

        // Abre o banco de dados Sled
        //
        // sled::open abre o banco no caminho especificado.
        // Se o banco não existir, ele será criado.
        let index = sled::open(index_path)?;

        Ok(Self { base_dir, index })
    }

    /// Retorna o diretório base do vault.
    ///
    /// # Retorna
    ///
    /// O caminho do diretório base onde os arquivos são armazenados.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Retorna o caminho completo para um arquivo baseado no seu hash.
    ///
    /// # Estrutura de Caminhos
    ///
    /// O caminho é construído usando os primeiros bytes do hash:
    ///
    /// ```text
    /// Hash: a1b2c3d4e5f6...
    /// Caminho: base_dir/a1/b2/a1b2c3d4e5f6....syntra
    /// ```
    ///
    /// # Por que esta estrutura?
    ///
    /// - Evita milhares de arquivos em um único diretório
    /// - A maioria dos sistemas de arquivos fica lento com muitos arquivos
    /// - A estrutura de 2 níveis (256x256) é um bom comprometimento
    ///
    /// # Parâmetros
    ///
    /// * `hash` - Hash BLAKE3 de 32 bytes do arquivo
    ///
    /// # Retorna
    ///
    /// O caminho completo onde o arquivo deve ser armazenado
    pub fn resolve_path(&self, hash: &[u8; 32]) -> PathBuf {
        // Converte o hash para hexadecimal
        //
        // hex::encode transforma os bytes em uma string hexadecimal.
        // Por exemplo, [0xAB, 0xCD] vira "abcd".
        let h = hex::encode(hash);

        // Pega os primeiros 2 caracteres (primeiro byte)
        let l1 = &h[0..2];

        // Pega os próximos 2 caracteres (segundo byte)
        let l2 = &h[2..4];

        // Monta o caminho completo
        //
        // join() adiciona partes ao caminho de forma portável.
        // format!() cria a string do nome do arquivo com extensão.
        self.base_dir.join(l1).join(l2).join(format!("{}.{}", h, ESSENCE_EXTENSION))
    }

    /// Retorna o caminho completo para um arquivo usando o nome completo (com timestamp único).
    ///
    /// # Parâmetros
    ///
    /// * `full_filename` - Nome completo do arquivo (ex: "a1b2...1234_1707839234567890123.syntra")
    ///
    /// # Retorna
    ///
    /// O caminho completo onde o arquivo está armazenado
    ///
    /// # Exemplo
    ///
    /// ```rust
    /// let path = vault.resolve_path_by_filename("a1b2c3d4_1707839234567890123.syntra");
    /// // path: base_dir/a1/b2/a1b2c3d4_1707839234567890123
    /// ```
    pub fn resolve_path_by_filename(&self, full_filename: &str) -> PathBuf {
        // Extrai o hash (primeiros 64 caracteres hex antes do '_')
        let hash_part = full_filename
            .split('_')
            .next()
            .unwrap_or(full_filename);

        // Usa os 2 primeiros bytes para o sharding
        let l1 = &hash_part[0..2];
        let l2 = &hash_part[2..4];

        // Monta o caminho completo com o nome original do arquivo
        self.base_dir.join(l1).join(l2).join(full_filename)
    }

    /// Garante que o diretório para um arquivo exista.
    ///
    /// # O que faz
    ///
    /// Calcula o caminho do arquivo e cria todos os diretórios pais
    /// se eles não existirem.
    ///
    /// # Parâmetros
    ///
    /// * `hash` - Hash BLAKE3 de 32 bytes do arquivo
    ///
    /// # Retorna
    ///
    /// O caminho completo onde o arquivo deve ser armazenado
    ///
    /// # Exemplo
    ///
    /// ```rust
    /// let hash = [0u8; 32]; // Hash zerado para exemplo
    /// let path = vault.ensure_dir(&hash)?;
    /// // path agora contém o caminho completo
    /// // e os diretórios pai foram criados se necessário
    /// ```
    pub fn ensure_dir(&self, hash: &[u8; 32]) -> Result<PathBuf> {
        // Calcula o caminho completo do arquivo
        let path = self.resolve_path(hash);

        // Cria os diretórios pais se não existirem
        //
        // parent() retorna o diretório pai do caminho.
        // Se path for "a1/b2/arquivo.syntra", parent retorna "a1/b2".
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        Ok(path)
    }

    /// Armazena o mapeamento de hash para nome de arquivo no índice.
    ///
    /// # O que faz
    ///
    /// Registra no banco Sled que um determinado hash corresponde
    /// a um arquivo específico. Isso permite encontrar rapidamente
    /// se um arquivo já existe no vault.
    ///
    /// # Parâmetros
    ///
    /// * `hash` - Hash BLAKE3 de 32 bytes (chave)
    /// * `filename` - Nome do arquivo (valor)
    ///
    /// # Retorna
    ///
    /// `Ok(())` se bem-sucedido, ou erro se falhar
    pub fn store_hash(&self, hash: &[u8; 32], filename: &str) -> Result<()> {
        // Insere o hash e o nome do arquivo no banco Sled
        //
        // insert() adiciona ou atualiza uma entrada chave-valor.
        self.index.insert(hash, filename.as_bytes())?;

        // flush() garante que os dados foram escritos no disco.
        // Isso é importante para não perder dados em caso de queda de energia.
        self.index.flush()?;

        Ok(())
    }

    /// Busca um nome de arquivo pelo seu hash.
    ///
    /// # O que faz
    ///
    /// Consulta o índice Sled para verificar se um hash já existe
    /// no vault. Isso é usado para deduplicação - se o hash já existe,
    /// não precisamos processar o arquivo novamente.
    ///
    /// # Parâmetros
    ///
    /// * `hash` - Hash BLAKE3 de 32 bytes a ser buscado
    ///
    /// # Retorna
    ///
    /// `Ok(Some(nome))` se o hash existir, `Ok(None)` se não existir
    pub fn find_by_hash(&self, hash: &[u8; 32]) -> Result<Option<String>> {
        // Busca o hash no índice Sled
        //
        // get() retorna None se a chave não existir.
        // Se existir, retorna Some(&[u8]) com o valor.
        let res = self.index.get(hash)?;

        // Converte o resultado para Option<String>
        //
        // map() transforma o valor se ele existir.
        // from_utf8_lossy() converte bytes para String, substituindo
        // caracteres inválidos por '?' se necessário.
        Ok(res.map(|v| String::from_utf8_lossy(&v).to_string()))
    }

    /// Remove um hash do índice.
    ///
    /// # O que faz
    ///
    /// Remove a entrada do índice Sled. Isso é usado quando um arquivo
    /// é deletado do vault.
    ///
    /// # Parâmetros
    ///
    /// * `hash` - Hash BLAKE3 de 32 bytes a ser removido
    ///
    /// # Retorna
    ///
    /// `Ok(())` se bem-sucedido, ou erro se falhar
    pub fn remove_hash(&self, hash: &[u8; 32]) -> Result<()> {
        // Remove a entrada do índice
        self.index.remove(hash)?;

        // Garante que a alteração foi escrita no disco
        self.index.flush()?;

        Ok(())
    }

    /// Retorna um iterador sobre todos os hashes no índice.
    ///
    /// # O que faz
    ///
    /// Cria um iterador que percorre todas as entradas do índice Sled.
    /// Útil para reconstruir o estado ou fazer auditoria.
    ///
    /// # Retorna
    ///
    /// Um iterador que yields tuplas (hash, nome_do_arquivo)
    pub fn iter_hashes(&self) -> impl Iterator<Item = Result<([u8; 32], String)>> {
        // iter() cria um iterador sobre todas as entradas do banco
        self.index.iter().map(|item| {
            // Cada item é um Result<(Vec<u8>, IVec)> que precisamos processar
            let (k, v) = item?;

            // Cria um array de 32 bytes para o hash
            let mut hash = [0u8; 32];

            // Copia os bytes da chave para o array
            //
            // Só copia se a chave tiver exatamente 32 bytes.
            // Se tiver menos, o hash permanece com zeros.
            if k.len() == 32 {
                hash.copy_from_slice(&k);
            }

            // Converte o valor para String
            Ok((hash, String::from_utf8_lossy(&v).to_string()))
        })
    }
}
