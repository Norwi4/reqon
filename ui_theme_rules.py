"""Scoped component rules share one window stylesheet and one theme update."""

COMPONENT_STYLE = '''
QWidget#settingsOverlay QFrame#settingsCard {background:#212121;border-radius:7px;}
QWidget#settingsOverlay QFrame#settingsNavigation {background:#262626;border-top-left-radius:7px;border-bottom-left-radius:7px;}
QWidget#settingsOverlay QWidget#proxySettings, QWidget#settingsOverlay QWidget#generalSettings, QWidget#settingsOverlay QWidget#themesSettings {background:#212121;}
QWidget#settingsOverlay QPushButton#settingsNavItem {border:0;text-align:left;background:transparent;padding:6px 10px;font-size:12px;font-weight:500;}
QWidget#settingsOverlay QPushButton#settingsNavItem:checked {background:#3b3b3b;color:#f2f2f2;}
QWidget#settingsOverlay QPushButton#settingsNavItem:disabled {color:#8e8e8e;}
QWidget#settingsOverlay QLabel#settingsHeading {font-size:18px;font-weight:600;}
QWidget#settingsOverlay QLabel#settingsSection {font-size:13px;font-weight:600;}
QWidget#settingsOverlay QLabel#settingsDescription {font-size:12px;color:#9b9b9b;}
QWidget#settingsOverlay QFrame#proxySetting {border-bottom:1px solid #2d2d2d;}
QWidget#settingsOverlay QLabel#proxyValidation {font-size:11px;color:#ed968d;}
QWidget#settingsOverlay QLineEdit, QWidget#settingsOverlay QComboBox, QWidget#settingsOverlay QPlainTextEdit, QWidget#settingsOverlay QSpinBox {font-size:12px;border:1px solid #555;border-radius:3px;background:#262626;}
QWidget#settingsOverlay QSpinBox {min-height:28px;padding-left:8px;}
QWidget#settingsOverlay QRadioButton {font-size:12px;}
QWidget#settingsOverlay QLineEdit:disabled {border-color:#333;color:#777;}
QWidget#settingsOverlay QCheckBox {font-size:12px;}
QWidget#settingsOverlay QToolButton#settingsClose {background:transparent;border:0;}
QWidget#settingsOverlay QToolButton#settingsClose:hover {background:#343434;}

QWidget#grpcSettings { background:#1f1f1f; }
QWidget#grpcSettings QLabel { font-size:12px; color:#aab2bd; }
QWidget#grpcSettings QLabel#settingTitle { font-weight:600; }
QWidget#grpcSettings QLineEdit { background:#1f1f1f; border:1px solid #363636; border-radius:3px;
            font-size:12px; padding:4px 8px; color:#f5f7fa; }
QWidget#grpcSettings QLineEdit:focus { border-color:#6a6a6a; }

QWidget#themesSettings QLabel#themeActive {color:#aab2bd;background:#343434;border-radius:3px;padding:2px 4px;font-size:9px;font-weight:600;}
'''
