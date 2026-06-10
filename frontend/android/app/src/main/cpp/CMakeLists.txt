cmake_minimum_required(VERSION 3.22.1)

project(continuum_android)

add_library(continuum_jni SHARED continuum_jni.cpp)

add_library(continuum_core STATIC IMPORTED)
set_target_properties(
    continuum_core
    PROPERTIES
    IMPORTED_LOCATION
    "${CMAKE_CURRENT_SOURCE_DIR}/../rustLibs/${ANDROID_ABI}/libcontinuum_core.a"
)

target_link_libraries(continuum_jni continuum_core log dl m unwind)
