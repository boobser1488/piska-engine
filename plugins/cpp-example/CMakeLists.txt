cmake_minimum_required(VERSION 3.16)
project(piska_vignette LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Заголовок C ABI генерирует cbindgen при сборке крейта engine-cpp-bridge.
set(ENGINE_INCLUDE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../crates/engine-cpp-bridge/include"
    CACHE PATH "Каталог с engine_api.h")

add_library(piska_vignette SHARED plugin.cpp)
target_include_directories(piska_vignette PRIVATE "${ENGINE_INCLUDE_DIR}")

# Наружу виден только engine_plugin_entry.
set_target_properties(piska_vignette PROPERTIES
    CXX_VISIBILITY_PRESET hidden
    VISIBILITY_INLINES_HIDDEN ON)

if(MSVC)
    target_compile_options(piska_vignette PRIVATE /W4 /utf-8)
else()
    target_compile_options(piska_vignette PRIVATE -Wall -Wextra)
endif()

if(WIN32)
    # На Windows движок ищет `piska_vignette.dll` — без префикса `lib`, который добавляет MinGW.
    set_target_properties(piska_vignette PROPERTIES PREFIX "")
endif()
if(MINGW)
    # Самодостаточная DLL: не тянуть libstdc++/libgcc из каталога MSYS.
    target_link_options(piska_vignette PRIVATE -static -static-libgcc -static-libstdc++)
endif()

# Кладём библиотеку в bin/ при любой конфигурации — там её ищет пример cpp_plugin.
foreach(config "" _DEBUG _RELEASE _RELWITHDEBINFO _MINSIZEREL)
    set_target_properties(piska_vignette PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY${config} "${CMAKE_CURRENT_SOURCE_DIR}/bin"
        LIBRARY_OUTPUT_DIRECTORY${config} "${CMAKE_CURRENT_SOURCE_DIR}/bin")
endforeach()
