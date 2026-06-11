#include <jni.h>

#include <cstdint>

extern "C" {
struct ContinuumMobileRuntime;

ContinuumMobileRuntime *continuum_mobile_create_preset(
    uint32_t preset_id,
    float sample_rate,
    uint32_t channels);
void continuum_mobile_destroy(ContinuumMobileRuntime *runtime);
uint32_t continuum_mobile_render(
    ContinuumMobileRuntime *runtime,
    float *output,
    uint32_t frames);
void continuum_mobile_set_paused(ContinuumMobileRuntime *runtime, bool paused);
void continuum_mobile_set_tempo(ContinuumMobileRuntime *runtime, float value);
void continuum_mobile_set_swing(ContinuumMobileRuntime *runtime, float value);
bool continuum_mobile_set_instrument_volume(
    ContinuumMobileRuntime *runtime,
    uint32_t instrument_id,
    float value);
}

static ContinuumMobileRuntime *from_handle(jlong handle) {
    return reinterpret_cast<ContinuumMobileRuntime *>(handle);
}

extern "C" JNIEXPORT jlong JNICALL
Java_com_continuum_app_ContinuumNative_createPreset(
    JNIEnv *,
    jobject,
    jint preset_id,
    jfloat sample_rate,
    jint channels) {
    auto *runtime = continuum_mobile_create_preset(
        static_cast<uint32_t>(preset_id),
        sample_rate,
        static_cast<uint32_t>(channels));
    return reinterpret_cast<jlong>(runtime);
}

extern "C" JNIEXPORT void JNICALL
Java_com_continuum_app_ContinuumNative_destroy(
    JNIEnv *,
    jobject,
    jlong runtime) {
    continuum_mobile_destroy(from_handle(runtime));
}

extern "C" JNIEXPORT jint JNICALL
Java_com_continuum_app_ContinuumNative_render(
    JNIEnv *env,
    jobject,
    jlong runtime,
    jfloatArray output,
    jint frames) {
    if (runtime == 0 || output == nullptr || frames <= 0) {
        return 0;
    }

    jboolean is_copy = JNI_FALSE;
    jfloat *samples = env->GetFloatArrayElements(output, &is_copy);
    if (samples == nullptr) {
        return 0;
    }

    uint32_t rendered = continuum_mobile_render(
        from_handle(runtime),
        samples,
        static_cast<uint32_t>(frames));
    env->ReleaseFloatArrayElements(output, samples, 0);
    return static_cast<jint>(rendered);
}

extern "C" JNIEXPORT void JNICALL
Java_com_continuum_app_ContinuumNative_setPaused(
    JNIEnv *,
    jobject,
    jlong runtime,
    jboolean paused) {
    continuum_mobile_set_paused(from_handle(runtime), paused == JNI_TRUE);
}

extern "C" JNIEXPORT void JNICALL
Java_com_continuum_app_ContinuumNative_setTempo(
    JNIEnv *,
    jobject,
    jlong runtime,
    jfloat value) {
    continuum_mobile_set_tempo(from_handle(runtime), value);
}

extern "C" JNIEXPORT void JNICALL
Java_com_continuum_app_ContinuumNative_setSwing(
    JNIEnv *,
    jobject,
    jlong runtime,
    jfloat value) {
    continuum_mobile_set_swing(from_handle(runtime), value);
}

extern "C" JNIEXPORT jboolean JNICALL
Java_com_continuum_app_ContinuumNative_setInstrumentVolume(
    JNIEnv *,
    jobject,
    jlong runtime,
    jint instrument_id,
    jfloat value) {
    return continuum_mobile_set_instrument_volume(
               from_handle(runtime),
               static_cast<uint32_t>(instrument_id),
               value)
               ? JNI_TRUE
               : JNI_FALSE;
}
