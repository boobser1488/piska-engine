//! Выравнивание времени кадра под частоту монитора.
//!
//! С вертикальной синхронизацией кадры показываются ровно через период монитора, но время
//! между вызовами кадра, измеренное часами, гуляет на миллисекунду-другую (планировщик,
//! сборка мусора драйвера). Если двигать мир на измеренное время, камера за одинаковые на
//! экране кадры проходит то 15, то 18 миллисекунд пути — картинка идёт мелкими рывками при
//! «нормальном» FPS. Фильтр заменяет измеренное время на целое число периодов монитора,
//! пока измерение с ними согласуется, и понемногу возвращает накопившуюся разницу с
//! настоящими часами, чтобы игровое время не уплывало.

use web_time::{Duration, Instant};

/// Фильтр времени кадра. Один на окно.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTimeFilter {
    /// Сколько настоящего времени ещё не отдано миру (может быть отрицательным), с.
    drift: f32,
}

impl FrameTimeFilter {
    /// Самое большое число пропущенных подряд периодов, которое ещё выравнивается.
    const MAX_PERIODS: f32 = 4.0;
    /// Насколько измерение может отличаться от целого числа периодов (в долях периода).
    const TOLERANCE: f32 = 0.25;
    /// Какую долю периода можно вернуть часам за один кадр.
    const CATCH_UP: f32 = 0.02;

    pub fn new() -> Self {
        Self::default()
    }

    /// Время кадра для мира. `raw` — измеренное часами; `refresh_period` — период монитора
    /// в секундах, когда кадры идут с вертикальной синхронизацией (`None` — без неё:
    /// выравнивать не под что, возвращается `raw`).
    pub fn filter(&mut self, raw: f32, refresh_period: Option<f32>) -> f32 {
        let Some(period) = refresh_period.filter(|p| p.is_finite() && *p > 1e-4) else {
            self.drift = 0.0;
            return raw;
        };
        if !raw.is_finite() || raw <= 0.0 {
            return raw;
        }
        let periods = (raw / period).round();
        let in_step = (1.0..=Self::MAX_PERIODS).contains(&periods) && (raw - periods * period).abs() <= Self::TOLERANCE * period;
        if !in_step {
            // Кадры идут не в такт (нет синхронизации на деле, долгая пауза, перетаскивание
            // окна): измеренное время — лучшее, что есть.
            self.drift = 0.0;
            return raw;
        }
        let even = periods * period;
        self.drift += raw - even;
        let correction = self.drift.clamp(-Self::CATCH_UP * period, Self::CATCH_UP * period);
        self.drift -= correction;
        even + correction
    }
}

/// Ровная подача кадров: когда начинать следующий кадр.
///
/// С вертикальной синхронизацией цикл «нарисовал — сразу рисуй следующий, монитор сам
/// притормозит» не даёт ровных кадров: драйвер держит очередь и отпускает её пачками
/// (замер на GTX 1050 Ti, 75 Гц: кадры начинались через 3, 10 и 27 мс по кругу). Ввод и
/// время берутся в начале кадра, то есть в эти неровные моменты, а экран показывает кадры
/// через равные промежутки — камера поворачивается и летит рывками при «нормальном» FPS.
/// Поэтому кадры начинаются по своим часам, раз в период монитора; небольшое расхождение
/// часов с монитором гасит очередь кадров.
#[derive(Clone, Copy, Debug, Default)]
pub struct FramePacer {
    /// Когда по плану начинается ближайший кадр.
    planned: Option<Instant>,
}

impl FramePacer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Кадр начался в момент `now`; возвращает, когда начинать следующий. Если кадр начался
    /// почти по плану, следующий отсчитывается от плана (ошибки пробуждения не копятся);
    /// если сильно не по плану (простой, долгий кадр) — от `now`.
    pub fn frame_started(&mut self, now: Instant, period: Duration) -> Instant {
        let base = match self.planned {
            Some(planned) => {
                let off = if now >= planned { now - planned } else { planned - now };
                if off * 2 < period {
                    planned
                } else {
                    now
                }
            }
            None => now,
        };
        let next = base + period;
        self.planned = Some(next);
        next
    }
}

/// Подача кадров по своим часам включена (выключить для сравнения: `ENGINE_PACING=off`).
pub fn pacing_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("ENGINE_PACING").map_or(true, |v| v != "off" && v != "0"))
}

/// Дождаться момента точно: грубо — сном, последние полторы миллисекунды — вращением
/// (таймер системы на столько может ошибиться, а ровность кадров этого не прощает).
#[cfg(not(target_arch = "wasm32"))]
pub fn wait_until(deadline: Instant) {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let left = deadline - now;
        if left > Duration::from_micros(1800) {
            std::thread::sleep(left - Duration::from_micros(1500));
        } else {
            std::hint::spin_loop();
        }
    }
}

/// Запись интервалов между началами кадров в файл (`ENGINE_TRACE_FRAMES=<файл>`): по ней
/// видно, ровно ли идут кадры на настоящем окне. `raw_dt` — измеренное время с прошлого
/// кадра, `dt` — то, на которое подвинулся мир.
#[cfg(not(target_arch = "wasm32"))]
pub fn trace_frame(raw_dt: f32, dt: f32) {
    use std::io::Write;
    static SINK: std::sync::OnceLock<Option<std::sync::Mutex<std::fs::File>>> = std::sync::OnceLock::new();
    let sink = SINK.get_or_init(|| {
        let path = std::env::var_os("ENGINE_TRACE_FRAMES")?;
        let mut file = std::fs::File::create(path).ok()?;
        writeln!(file, "raw_dt_ms,dt_ms").ok()?;
        Some(std::sync::Mutex::new(file))
    });
    if let Some(Ok(mut file)) = sink.as_ref().map(|s| s.lock()) {
        let _ = writeln!(file, "{:.3},{:.3}", raw_dt * 1000.0, dt * 1000.0);
    }
}

/// Период монитора в секундах по его частоте в миллигерцах (как её отдаёт winit).
pub fn refresh_period(millihertz: Option<u32>) -> Option<f32> {
    millihertz.filter(|m| *m >= 20_000).map(|m| 1000.0 / m as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: f32 = 1.0 / 60.0;

    #[test]
    fn jittery_vsync_frames_become_even_and_keep_real_time() {
        let mut filter = FrameTimeFilter::new();
        let jitter = [0.0150, 0.0183, 0.0162, 0.0171, 0.0158, 0.0176];
        let (mut real, mut given) = (0.0f64, 0.0f64);
        let (mut lo, mut hi) = (f32::MAX, 0.0f32);
        for i in 0..6000 {
            let raw = jitter[i % jitter.len()];
            let dt = filter.filter(raw, Some(PERIOD));
            real += raw as f64;
            given += dt as f64;
            lo = lo.min(dt);
            hi = hi.max(dt);
        }
        // Измерения гуляли на ±10 %; мир получает шаги в пределах ±2 % периода…
        assert!(lo >= PERIOD * 0.979 && hi <= PERIOD * 1.021, "steps {lo}..{hi}");
        // …и за 100 секунд не отстаёт от часов больше чем на кадр.
        assert!((real - given).abs() < PERIOD as f64, "drift {}", real - given);
    }

    #[test]
    fn a_slightly_different_real_refresh_rate_does_not_accumulate_error() {
        // Монитор назвался 60 Гц, а показывает 59.94: фильтр идёт в ногу с часами.
        let mut filter = FrameTimeFilter::new();
        let (mut real, mut given) = (0.0f64, 0.0f64);
        for _ in 0..60 * 600 {
            let raw = 1.0 / 59.94;
            given += filter.filter(raw, Some(PERIOD)) as f64;
            real += raw as f64;
        }
        assert!((real - given).abs() < PERIOD as f64, "drift after 10 minutes: {}", real - given);
    }

    #[test]
    fn a_dropped_frame_counts_as_two_periods() {
        let mut filter = FrameTimeFilter::new();
        for _ in 0..10 {
            filter.filter(PERIOD, Some(PERIOD));
        }
        let dt = filter.filter(0.0338, Some(PERIOD));
        assert!((dt - 2.0 * PERIOD).abs() < 0.001, "{dt}");
    }

    #[test]
    fn frames_that_do_not_follow_the_monitor_pass_through() {
        let mut filter = FrameTimeFilter::new();
        // Без синхронизации.
        assert_eq!(filter.filter(0.0031, None), 0.0031);
        // Синхронизация заявлена, но кадры вдвое чаще периода (её нет на деле).
        assert_eq!(filter.filter(0.0074, Some(PERIOD)), 0.0074);
        // Долгая пауза.
        assert_eq!(filter.filter(0.25, Some(PERIOD)), 0.25);
        // Не число.
        assert!(filter.filter(f32::NAN, Some(PERIOD)).is_nan());
    }

    #[test]
    fn frames_are_planned_one_period_apart_whatever_the_wake_up_jitter() {
        let period = Duration::from_micros(13_333);
        let mut pacer = FramePacer::new();
        let start = Instant::now();
        // Кадры просыпаются с ошибкой до ±2 мс вокруг плана: план остаётся ровной сеткой.
        let mut next = pacer.frame_started(start, period);
        for (i, jitter_us) in [900i64, -1500, 2000, 0, -700, 1800, -2000, 300].into_iter().enumerate() {
            assert_eq!(next, start + period * (i as u32 + 1), "frame {i}");
            let woke = if jitter_us >= 0 { next + Duration::from_micros(jitter_us as u64) } else { next - Duration::from_micros((-jitter_us) as u64) };
            next = pacer.frame_started(woke, period);
        }
        // Долгий кадр или простой: сетка начинается заново от настоящего момента.
        let late = next + Duration::from_millis(40);
        assert_eq!(pacer.frame_started(late, period), late + period);
    }

    #[test]
    fn the_period_comes_from_millihertz() {
        assert!((refresh_period(Some(60_000)).unwrap() - PERIOD).abs() < 1e-6);
        assert!((refresh_period(Some(143_998)).unwrap() - 1.0 / 143.998).abs() < 1e-6);
        assert_eq!(refresh_period(None), None);
        assert_eq!(refresh_period(Some(0)), None);
    }
}
