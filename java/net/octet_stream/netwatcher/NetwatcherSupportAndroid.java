package net.octet_stream.netwatcher;

import android.content.Context;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.net.NetworkRequest;
import android.util.Log;

/**
 * Support class enabling the Rust crate netwatcher to monitor network interface changes,
 * functionality which is not available in the NDK. This class will be instantiated automatically
 * via JNI and should not be used directly.
 */
public class NetwatcherSupportAndroid {
    private static final String TAG = "NetwatcherSupportAndroid";

    private ConnectivityManager connectivityManager;
    private ConnectivityManager.NetworkCallback networkCallback;

    /**
     * Create a Java-based network watcher. Invoked only via JNI.
     * @param context Activity or other Context that can be used to get system services
     */
    public NetwatcherSupportAndroid(Context context) {
        this.connectivityManager = (ConnectivityManager) context.getSystemService(Context.CONNECTIVITY_SERVICE);
    }

    /**
     * Start monitoring network interface changes
     */
    public void startInterfaceWatch() {
        if (connectivityManager == null) {
            Log.e(TAG, "ConnectivityManager not available");
            return;
        }
        NetworkRequest networkRequest = new NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_RESTRICTED)
                .build();
        networkCallback = new ConnectivityManager.NetworkCallback() {
            @Override
            public void onAvailable(Network network) {
                netwatcherInterfacesDidChange();
            }

            @Override
            public void onLost(Network network) {
                netwatcherInterfacesDidChange();
            }

            @Override
            public void onCapabilitiesChanged(Network network, NetworkCapabilities networkCapabilities) {
                netwatcherInterfacesDidChange();
            }

            @Override
            public void onLinkPropertiesChanged(Network network, android.net.LinkProperties linkProperties) {
                netwatcherInterfacesDidChange();
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
        }
    }

    /**
     * Callback that will be implemented via RegisterNatives in Rust
     */
    private native void netwatcherInterfacesDidChange();
}
