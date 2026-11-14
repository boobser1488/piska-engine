//! Выравнивание времени кадра под частоту монитора.
//!
//! С вертикальной синхронизацией кадры показываются ровно через период монитора, но время
//! между вызовами кадра, измеренное часами, гуляет на миллисекунду-другую (планировщик,
//! сборка мусора драйвера). Если двигать мир на измеренное время, камера за одинаковые на
//! экране кадры проходит то 15, то 18 миллисекунд пути — картинка идёт мелкими рывками при
//! «нормальном» FPS. Фильтр заменяет измеренное время на целое число периодов монитора,
//! пока измерение с ними согласуется, и понемногу возвращает накопившуюся разницу с
//! настоящими часами, чтобы игровое время не уплывало.

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
    fn the_period_comes_from_millihertz() {
        assert!((refresh_period(Some(60_000)).unwrap() - PERIOD).abs() < 1e-6);
        assert!((refresh_period(Some(143_998)).unwrap() - 1.0 / 143.998).abs() < 1e-6);
        assert_eq!(refresh_period(None), None);
        assert_eq!(refresh_period(Some(0)), None);
    }
}
