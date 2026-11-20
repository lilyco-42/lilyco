package top.lain42.workbench;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;

public class MainActivity extends Activity {
    private static final String START_URL = "https://dev.lain42.top/";
    private static final String FALLBACK_URL = "http://123.60.38.191/";
    private static final int FILE_CHOOSER_REQ = 1001;

    private WebView webView;
    private ValueCallback<Uri[]> filePathCallback;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        webView = new WebView(this);
        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setDatabaseEnabled(true);
        settings.setAllowFileAccess(true);
        settings.setLoadWithOverviewMode(true);
        settings.setUseWideViewPort(true);
        settings.setSupportZoom(false);
        settings.setBuiltInZoomControls(false);
        settings.setDisplayZoomControls(false);

        webView.setWebViewClient(new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                return false;
            }

            @Override
            public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
                if (request != null && request.isForMainFrame()) {
                    String desc = error != null ? String.valueOf(error.getDescription()) : "网络异常";
                    String html = "<!doctype html><html lang='zh-CN'><head><meta charset='utf-8'>"
                        + "<meta name='viewport' content='width=device-width,initial-scale=1'>"
                        + "<style>body{font-family:sans-serif;margin:0;padding:32px 20px;background:#fff;color:#09090b}"
                        + ".card{border:1px solid #e4e4e7;border-radius:10px;padding:20px;max-width:420px;margin:40px auto}"
                        + "h2{margin:0 0 8px;font-size:17px}p{color:#71717a;font-size:13px;line-height:1.6}"
                        + ".btn{display:block;width:100%;padding:12px;margin-top:10px;border-radius:8px;border:1px solid #e4e4e7;font-size:14px;font-weight:600;text-align:center;text-decoration:none;box-sizing:border-box}"
                        + ".pri{background:#18181b;color:#fff;border-color:#18181b}.sec{background:#fff;color:#09090b}</style></head>"
                        + "<body><div class='card'><h2>暂时无法连接服务器</h2>"
                        + "<p>原因：" + desc + "<br>请检查网络或 DNS（Waydroid 容器需开启网络/DNS 转发）。</p>"
                        + "<a class='btn pri' href='" + START_URL + "'>重新加载 dev.lain42.top</a>"
                        + "<a class='btn sec' href='" + FALLBACK_URL + "'>尝试直连 IP (123.60.38.191)</a>"
                        + "</div></body></html>";
                    view.loadDataWithBaseURL(null, html, "text/html", "utf-8", null);
                }
            }
        });

        webView.setWebChromeClient(new WebChromeClient() {
            @Override
            public boolean onShowFileChooser(
                    WebView view,
                    ValueCallback<Uri[]> callback,
                    FileChooserParams params) {
                if (filePathCallback != null) {
                    filePathCallback.onReceiveValue(null);
                }
                filePathCallback = callback;
                Intent intent = params.createIntent();
                try {
                    startActivityForResult(intent, FILE_CHOOSER_REQ);
                } catch (Exception e) {
                    filePathCallback = null;
                    return false;
                }
                return true;
            }
        });

        setContentView(webView);
        webView.loadUrl(START_URL);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        if (requestCode == FILE_CHOOSER_REQ && filePathCallback != null) {
            Uri[] results = WebChromeClient.FileChooserParams.parseResult(resultCode, data);
            filePathCallback.onReceiveValue(results);
            filePathCallback = null;
            return;
        }
        super.onActivityResult(requestCode, resultCode, data);
    }

    @Override
    public void onBackPressed() {
        if (webView != null && webView.canGoBack()) {
            webView.goBack();
        } else {
            super.onBackPressed();
        }
    }
}
