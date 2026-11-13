cmake_minimum_required(VERSION 3.16)
project(piska_gameplay LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# engine_api.h генерирует cbindgen при сборке крейта engine-cpp-bridge;
# engine_component.hpp — обёртка для C++ рядом с ним.
# Путь без «..»: на Windows каждая лишняя пара символов приближает к пределу в 260.
get_filename_component(ENGINE_DEFAULT_INCLUDE_DIR
    "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/engine-cpp-bridge/include" ABSOLUTE)
set(ENGINE_INCLUDE_DIR "${ENGINE_DEFAULT_INCLUDE_DIR}"
    CACHE PATH "Каталог с engine_api.h и engine_component.hpp")

# Каждый .cpp в scripts/ — отдельный скрипт; новые файлы подхватываются при сборке.
file(GLOB_RECURSE PISKA_SCRIPTS CONFIGURE_DEPENDS "${CMAKE_CURRENT_SOURCE_DIR}/scripts/*.cpp")
add_library(piska_gameplay SHARED gameplay.cpp ${PISKA_SCRIPTS})
target_include_directories(piska_gameplay PRIVATE "${ENGINE_INCLUDE_DIR}")

# Наружу виден только engine_plugin_entry.
set_target_properties(piska_gameplay PROPERTIES
    CXX_VISIBILITY_PRESET hidden
    VISIBILITY_INLINES_HIDDEN ON)

if(MSVC)
    target_compile_options(piska_gameplay PRIVATE /W4 /utf-8)
else()
    # offsetof для классов с виртуальными методами поддержан GCC и Clang, но с предупреждением.
    target_compile_options(piska_gameplay PRIVATE -Wall -Wextra -Wno-invalid-offsetof)
endif()

if(WIN32)
    # На Windows движок ищет `piska_gameplay.dll` — без префикса `lib`, который добавляет MinGW.
    set_target_properties(piska_gameplay PROPERTIES PREFIX "")
endif()
if(MINGW)
    # Самодостаточная DLL: не тянуть libstdc++/libgcc из каталога MSYS.
    target_link_options(piska_gameplay PRIVATE -static -static-libgcc -static-libstdc++)
endif()

# Кладём библиотеку в bin/ при любой конфигурации — там её ищут примеры.
foreach(config "" _DEBUG _RELEASE _RELWITHDEBINFO _MINSIZEREL)
    set_target_properties(piska_gameplay PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY${config} "${CMAKE_CURRENT_SOURCE_DIR}/bin"
        LIBRARY_OUTPUT_DIRECTORY${config} "${CMAKE_CURRENT_SOURCE_DIR}/bin")
endforeach()
