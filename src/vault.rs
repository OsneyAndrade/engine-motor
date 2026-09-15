
use std::path::{Path, PathBuf};
use std::fs;
use anyhow::Result;
use sled::Db;
use crate::config::ESSENCE_EXTENSION;

pub struct VaultManager {
    base_dir: PathBuf,
    index: Db,
}

impl VaultManager {

    pub fn new(base_dir: impl AsRef<Path>, index_path: impl AsRef<Path>) -> Result<Self> {

        let base_dir = base_dir.as_ref().to_path_buf();
        if !base_dir.exists() {
            fs::create_dir_all(&base_dir)?;
        }
        let index = sled::open(index_path)?;

        Ok(Self { base_dir, index })
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }


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


    pub fn find_by_hash(&self, hash: &[u8; 32]) -> Result<Option<String>> {

        let res = self.index.get(hash)?;

        Ok(res.map(|v| String::from_utf8_lossy(&v).to_string()))
    }

    pub fn remove_hash(&self, hash: &[u8; 32]) -> Result<()> {
        // Remove a entrada do índice
        self.index.remove(hash)?;

        // Garante que a alteração foi escrita no disco
        self.index.flush()?;

        Ok(())
    }

    pub fn iter_hashes(&self) -> impl Iterator<Item = Result<([u8; 32], String)>> {
        // iter() cria um iterador sobre todas as entradas do banco
        self.index.iter().map(|item| {
            // Cada item é um Result<(Vec<u8>, IVec)> que precisamos processar
            let (k, v) = item?;

            // Cria um array de 32 bytes para o hash
            let mut hash = [0u8; 32];

            if k.len() == 32 {
                hash.copy_from_slice(&k);
            }

            // Converte o valor para String
            Ok((hash, String::from_utf8_lossy(&v).to_string()))
        })
    }
}
