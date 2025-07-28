package net.octet_stream.netwatcher.netwatchertestapp

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import net.octet_stream.netwatcher.netwatchertestapp.ui.theme.NetwatcherTestAppTheme

class MainActivity : ComponentActivity() {
    private var displayText by mutableStateOf("Initializing...")
    
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        
        // Pass down Android context via the app-specific native Rust code
        setAndroidContext(this)
        
        // Start watching interfaces
        startWatching(object : InterfaceChangeCallback {
            override fun onInterfacesChanged(data: String) {
                runOnUiThread {
                    displayText = data
                }
            }
        })
        
        setContent {
            NetwatcherTestAppTheme {
                Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
                    InterfaceDisplayScreen(
                        interfaceData = displayText,
                        modifier = Modifier.padding(innerPadding)
                    )
                }
            }
        }
    }
    
    override fun onDestroy() {
        super.onDestroy()
        stopWatching()
    }
    
    private external fun setAndroidContext(context: android.content.Context)
    private external fun startWatching(callback: InterfaceChangeCallback)
    private external fun stopWatching()
    
    companion object {
        init {
            System.loadLibrary("netwatcher_app_native")
        }
    }
}

interface InterfaceChangeCallback {
    fun onInterfacesChanged(data: String)
}

@Composable
fun InterfaceDisplayScreen(interfaceData: String, modifier: Modifier = Modifier) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState())
    ) {
        Text(
            text = "Network Interfaces",
            style = MaterialTheme.typography.headlineMedium,
            modifier = Modifier.padding(bottom = 16.dp)
        )
        Card(
            modifier = Modifier.fillMaxWidth(),
            elevation = CardDefaults.cardElevation(defaultElevation = 4.dp)
        ) {
            Text(
                text = interfaceData,
                style = MaterialTheme.typography.bodyMedium,
                fontFamily = FontFamily.Monospace,
                modifier = Modifier.padding(16.dp)
            )
        }
    }
}