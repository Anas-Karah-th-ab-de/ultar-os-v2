include $(sort $(wildcard $(BR2_EXTERNAL_ULTAR_PATH)/package/*/*.mk))
# GStreamer-GL ohne Fenstersystem (WebKit verlangt die Bibliothek für Video): EGL ohne Oberfläche
GST1_PLUGINS_BASE_CONF_OPTS += -Dgl=enabled -Dgl_winsys=egl,surfaceless
# Zertifikate aus dem Bündel (keine p11-kit-Konfiguration auf dem Gerät nötig)
GNUTLS_CONF_OPTS += --with-default-trust-store-pkcs11= --with-default-trust-store-file=/usr/lib/ultar/web/etc/ssl/certs/ca-certificates.crt
