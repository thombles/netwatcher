package net.octet_stream.netwatcher.netwatcher_android;

import android.content.Context;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.net.NetworkRequest;
import android.util.Log;

/**
 * Support class enabling the Rust crate netwatch to monitor network interface changes,
 * functionality which is not available in the NDK. This class will be instantiated automatically
 * via JNI and should not be used directly.
 */
public class NetwatcherAndroidSupport {
    private static final String TAG = "NetwatcherAndroid";

    private ConnectivityManager connectivityManager;
    private NetworkRequest networkRequest;
    private ConnectivityManager.NetworkCallback networkCallback;
    private long nativeCallbackPtr;

    static {
        System.loadLibrary("netwatcher_android");
    }

    /**
     * Create a Java-based network watcher. Invoked only via JNI.
     * @param context Activity or other Context that can be used to get system services
     */
    public NetwatcherAndroidSupport(Context context) {
        this.connectivityManager = (ConnectivityManager) context.getSystemService(Context.CONNECTIVITY_SERVICE);
    }

    /**
     * Start monitoring network interface changes
     * @param callbackPtr Native function pointer to call when interfaces change
     */
    public void startInterfaceWatch(long callbackPtr) {
        this.nativeCallbackPtr = callbackPtr;

        if (connectivityManager == null) {
            Log.e(TAG, "ConnectivityManager not available");
            return;
        }
        networkRequest = new NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_RESTRICTED)
                .build();
        networkCallback = new ConnectivityManager.NetworkCallback() {
            @Override
            public void onAvailable(Network network) {
                callNativeCallback(nativeCallbackPtr);
            }

            @Override
            public void onLost(Network network) {
                callNativeCallback(nativeCallbackPtr);
            }

            @Override
            public void onCapabilitiesChanged(Network network, NetworkCapabilities networkCapabilities) {
                callNativeCallback(nativeCallbackPtr);
            }

            @Override
            public void onLinkPropertiesChanged(Network network, android.net.LinkProperties linkProperties) {
                callNativeCallback(nativeCallbackPtr);
            }
        };

        connectivityManager.registerNetworkCallback(networkRequest, networkCallback);
    }

    /**
     * Stop monitoring network interface changes
     */
    public void stopInterfaceWatch() {
        if (connectivityManager != null && networkCallback != null) {
            connectivityManager.unregisterNetworkCallback(networkCallback);
            networkCallback = null;
            networkRequest = null;
        }
    }

    private native void callNativeCallback(long callbackPtr);
}