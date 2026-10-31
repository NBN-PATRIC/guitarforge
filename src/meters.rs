use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicU32};

pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }

    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed);
    }

    /// Só para valores >= 0: nessa faixa a ordem dos bits do f32 é a mesma dos valores.
    pub fn raise(&self, v: f32) {
        self.0.fetch_max(v.max(0.0).to_bits(), Relaxed);
    }

    pub fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Relaxed))
    }
}

/// Medidores escritos pelo áudio e pela análise, lidos pela interface. Sem trava.
pub struct Meters {
    pub in_peak: AtomicF32,
    /// Limiar escolhido pelo gate automático (dB; -120 enquanto ainda mede).
    pub gate_auto_db: AtomicF32,
    /// Rede elétrica detectada pelo zumbido do cabo (50 ou 60 Hz).
    pub mains_hz: AtomicF32,
    pub out_peak: AtomicF32,
    pub gate_open: AtomicBool,
    pub in_block: AtomicU32,
    pub out_block: AtomicU32,
    pub ring_fill: AtomicU32,
    /// Amostras enfileiradas no dispositivo de saída no modo exclusivo (0 no compartilhado).
    pub out_queue: AtomicU32,
    pub underruns: AtomicU32,
    /// Amostras jogadas fora pela saída porque o anel encheu demais (estalo que não conta como falha).
    pub ring_drops: AtomicU32,
    /// Amostras pedidas pela saida exclusiva desde a abertura (o dispositivo tem de pedir a taxa dele por segundo).
    pub out_frames: std::sync::atomic::AtomicU64,
    /// Taxa (amostras/s) que a saída exclusiva pediu quando errou e passou para o compartilhado; 0 = não passou.
    pub out_fallback: AtomicU32,
    /// Maior intervalo entre dois blocos de entrada e maior tempo de processamento de um bloco (µs).
    pub in_gap_max_us: AtomicU32,
    pub proc_max_us: AtomicU32,
    pub device_error: AtomicBool,
    pub recording: AtomicBool,
    pub rec_dropped: AtomicU32,
    pub pitch_hz: AtomicF32,
    /// Frequência mais grave que o afinador procura; a interface ajusta pela afinação escolhida (baixo vai a ~22 Hz).
    pub pitch_fmin: AtomicF32,
    /// Frequência da corda escolhida no afinador (0 = automático).
    pub pitch_target: AtomicF32,
    pub in_rms_db: AtomicF32,
    pub noise_floor_db: AtomicF32,
    pub hum_db: AtomicF32,
}

impl Meters {
    pub fn new() -> Self {
        Self {
            in_peak: AtomicF32::new(0.0),
            gate_auto_db: AtomicF32::new(-120.0),
            mains_hz: AtomicF32::new(60.0),
            out_peak: AtomicF32::new(0.0),
            gate_open: AtomicBool::new(false),
            in_block: AtomicU32::new(0),
            out_block: AtomicU32::new(0),
            ring_fill: AtomicU32::new(0),
            out_queue: AtomicU32::new(0),
            underruns: AtomicU32::new(0),
            ring_drops: AtomicU32::new(0),
            out_frames: std::sync::atomic::AtomicU64::new(0),
            out_fallback: AtomicU32::new(0),
            in_gap_max_us: AtomicU32::new(0),
            proc_max_us: AtomicU32::new(0),
            device_error: AtomicBool::new(false),
            recording: AtomicBool::new(false),
            rec_dropped: AtomicU32::new(0),
            pitch_hz: AtomicF32::new(0.0),
            pitch_fmin: AtomicF32::new(60.0),
            pitch_target: AtomicF32::new(0.0),
            in_rms_db: AtomicF32::new(-120.0),
            noise_floor_db: AtomicF32::new(-120.0),
            hum_db: AtomicF32::new(-120.0),
        }
    }

    pub fn reset_engine_counters(&self) {
        self.underruns.store(0, Relaxed);
        self.ring_drops.store(0, Relaxed);
        self.out_fallback.store(0, Relaxed);
        self.in_gap_max_us.store(0, Relaxed);
        self.proc_max_us.store(0, Relaxed);
        self.device_error.store(false, Relaxed);
        self.ring_fill.store(0, Relaxed);
        self.pitch_hz.set(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raise_keeps_maximum_and_take_resets() {
        let a = AtomicF32::new(0.0);
        a.raise(0.25);
        a.raise(0.75);
        a.raise(0.5);
        assert_eq!(a.take(), 0.75);
        assert_eq!(a.get(), 0.0);
    }
}
