use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::config::BlockSettings;
use crate::metrics::EngineMetrics;
use crate::service::EngineError;

pub struct Budget {
    sem: Arc<Semaphore>,
    units: usize,
    block_size: u64,
    wait: Duration,
    retry_after_secs: u64,
    metrics: Arc<EngineMetrics>,
}

pub struct Slot {
    _unit: OwnedSemaphorePermit,
    _window: Option<OwnedSemaphorePermit>,
    bytes: u64,
    metrics: Arc<EngineMetrics>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.metrics
            .inflight_bytes
            .fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

impl Budget {
    pub fn new(settings: &BlockSettings, metrics: Arc<EngineMetrics>) -> Self {
        let units = settings.inflight_units();
        Budget {
            sem: Arc::new(Semaphore::new(units)),
            units,
            block_size: settings.block_size() as u64,
            wait: Duration::from_millis(settings.inflight_wait_ms),
            retry_after_secs: settings.retry_after_secs(),
            metrics,
        }
    }

    pub fn units(&self) -> usize {
        self.units
    }

    pub fn limit_bytes(&self) -> u64 {
        self.units as u64 * self.block_size
    }

    pub fn busy(&self) -> EngineError {
        EngineError::Busy {
            retry_after_secs: self.retry_after_secs,
        }
    }

    pub async fn acquire(
        &self,
        window: Option<Arc<Semaphore>>,
        admission: bool,
    ) -> Result<Slot, EngineError> {
        let window_permit = match window {
            Some(w) => Some(
                w.acquire_owned()
                    .await
                    .map_err(|_| EngineError::Overloaded)?,
            ),
            None => None,
        };

        let unit = if admission {
            match tokio::time::timeout(self.wait, self.sem.clone().acquire_owned()).await {
                Ok(Ok(p)) => p,
                Ok(Err(_)) => return Err(EngineError::Overloaded),
                Err(_) => return Err(self.busy()),
            }
        } else {
            self.sem
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| EngineError::Overloaded)?
        };

        let now = self
            .metrics
            .inflight_bytes
            .fetch_add(self.block_size, Ordering::Relaxed)
            + self.block_size;
        self.metrics
            .inflight_peak_bytes
            .fetch_max(now, Ordering::Relaxed);

        Ok(Slot {
            _unit: unit,
            _window: window_permit,
            bytes: self.block_size,
            metrics: self.metrics.clone(),
        })
    }

    #[cfg(test)]
    pub fn semaphore(&self) -> Arc<Semaphore> {
        self.sem.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(units_mb: usize, log2: u8, wait_ms: u64) -> (Budget, Arc<EngineMetrics>) {
        let settings = BlockSettings {
            block_log2: log2,
            max_inflight_mb: units_mb,
            inflight_wait_ms: wait_ms,
            ..BlockSettings::default()
        };
        let metrics = Arc::new(EngineMetrics::new());
        (Budget::new(&settings, metrics.clone()), metrics)
    }

    #[test]
    fn unidades_derivam_do_orcamento_e_do_tamanho_de_bloco() {
        let (b, _) = budget(8, 20, 100);
        assert_eq!(b.units(), 8);
        assert_eq!(b.limit_bytes(), 8 << 20);

        let (b, _) = budget(1, 12, 100);
        assert_eq!(b.units(), 256);

        let (b, _) = budget(1, 26, 100);
        assert_eq!(b.units(), 1, "o orçamento nunca fica abaixo de uma unidade");
    }

    #[tokio::test]
    async fn gauge_sobe_ao_adquirir_e_desce_ao_soltar() {
        let (b, m) = budget(4, 20, 100);
        let a = b.acquire(None, true).await.unwrap();
        let c = b.acquire(None, true).await.unwrap();
        assert_eq!(m.inflight_bytes.load(Ordering::Relaxed), 2 << 20);
        drop(a);
        assert_eq!(m.inflight_bytes.load(Ordering::Relaxed), 1 << 20);
        drop(c);
        assert_eq!(m.inflight_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(m.inflight_peak_bytes.load(Ordering::Relaxed), 2 << 20);
    }

    #[tokio::test]
    async fn admissao_esgotada_devolve_busy_com_retry_after() {
        let (b, _) = budget(2, 20, 30);
        let _a = b.acquire(None, true).await.unwrap();
        let _c = b.acquire(None, true).await.unwrap();
        match b.acquire(None, true).await {
            Err(EngineError::Busy { retry_after_secs }) => assert_eq!(retry_after_secs, 1),
            Err(e) => panic!("esperava Busy, veio {e}"),
            Ok(_) => panic!("esperava Busy, mas adquiriu"),
        }
    }

    #[tokio::test]
    async fn aquisicao_sem_admissao_espera_ate_haver_vaga() {
        let (b, _) = budget(1, 20, 10);
        let b = Arc::new(b);
        let a = b.acquire(None, true).await.unwrap();
        let b2 = b.clone();
        let espera = tokio::spawn(async move { b2.acquire(None, false).await.is_ok() });
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(!espera.is_finished(), "sem admissão não há timeout");
        drop(a);
        assert!(espera.await.unwrap());
    }

    #[tokio::test]
    async fn janela_por_requisicao_limita_os_blocos_em_voo() {
        let (b, _) = budget(16, 20, 100);
        let janela = Arc::new(Semaphore::new(2));
        let _a = b.acquire(Some(janela.clone()), true).await.unwrap();
        let _c = b.acquire(Some(janela.clone()), true).await.unwrap();
        let terceira = tokio::time::timeout(
            Duration::from_millis(50),
            b.acquire(Some(janela.clone()), true),
        )
        .await;
        assert!(terceira.is_err(), "a janela de 2 blocos deveria segurar o terceiro");
    }
}
