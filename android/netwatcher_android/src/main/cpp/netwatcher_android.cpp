#include <jni.h>
#include <android/log.h>

typedef void (*InterfaceChangeCallback)();

extern "C" JNIEXPORT void JNICALL
Java_net_octet_1stream_netwatcher_netwatcher_1android_NetwatcherAndroidSupport_callNativeCallback(
        JNIEnv* env,
        jobject thiz,
        jlong callbackPtr) {
    if (callbackPtr != 0) {
        auto callback = reinterpret_cast<InterfaceChangeCallback>(callbackPtr);
        callback();
    }
}