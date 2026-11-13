//! Отладочные переключатели для разработчика (скриншоты, замер FPS, автопилот).
//!
//! Читаются из переменных окружения только на десктопе (`cfg`): в браузере и на
//! Android они всегда «выключены», поэтому переносимость кода не страдает.
//!
//! * `SWARM_SCREEN=menu|mode|options|wave<N>[_pause|_upgrade|_win|_lose]` — стартовый экран
//!   (`wave7` — забег с автопилотом, будто дошли до 7-й волны);
//! * `SWARM_STRESS=1` — нагрузочный режим: 400 врагов и 600 пуль;
//! * `SWARM_BOT=1` — автопилот играет за игрока.

#[derive(Clone, Debug, Default)]
pub struct Debug {
    pub screen: Option<String>,
    pub stress: bool,
    pub bot: bool,
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn read() -> Debug {
    let flag = |name: &str| std::env::var(name).is_ok_and(|v| v != "0" && !v.is_empty());
    let stress = flag("SWARM_STRESS");
    Debug {
        screen: if stress { Some("wave20".to_string()) } else { std::env::var("SWARM_SCREEN").ok().filter(|s| !s.is_empty()) },
        stress,
        bot: flag("SWARM_BOT") || stress,
    }
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn read() -> Debug {
    Debug::default()
}
