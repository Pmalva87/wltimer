package com.pcorreia.wltimer

import android.content.Intent
import android.os.Bundle
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts

class MainActivity : TauriActivity() {
  private var pendingContent: String? = null
  private lateinit var saveLauncher: ActivityResultLauncher<Intent>

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Timer app: never let the screen sleep while the app is in the foreground.
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    saveLauncher =
      registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val uri = result.data?.data
        val content = pendingContent
        pendingContent = null
        if (uri != null && content != null) {
          try {
            contentResolver.openOutputStream(uri, "wt")?.use { it.write(content.toByteArray()) }
          } catch (_: Exception) {
            // User can simply retry; nothing to clean up.
          }
        }
      }
  }

  override fun onWebViewCreate(webView: WebView) {
    webView.addJavascriptInterface(FileSaver(), "WltimerFiles")
  }

  /** JS bridge: lets the webview save a text file via the system file picker. */
  inner class FileSaver {
    @JavascriptInterface
    fun save(filename: String, content: String) {
      pendingContent = content
      val intent =
        Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
          addCategory(Intent.CATEGORY_OPENABLE)
          type = "text/markdown"
          putExtra(Intent.EXTRA_TITLE, filename)
        }
      runOnUiThread { saveLauncher.launch(intent) }
    }
  }
}
