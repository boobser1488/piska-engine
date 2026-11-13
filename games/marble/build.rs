//! Сборка C++-механизмов уровней: все `cpp/*.cpp` линкуются в крейт статически.
//! На wasm32 (веб) C++ не собирается — игра рассчитана на Windows, Linux и Android.

fn main() {
    engine_cpp_build::compile("cpp");
    // Значок приложения в exe на Windows (`assets/icon.png` игры или значок движка).
    engine_build::windows_icon();
}
