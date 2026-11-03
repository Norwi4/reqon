import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from PySide6.QtWidgets import QApplication
from PySide6.QtTest import QTest
from ui_pages import ResponsePanel
from general_settings import GeneralConfig
from response_format import prepare_response


class ResponsePagingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls): cls.app = QApplication.instance() or QApplication([])
    def setUp(self): self.panel = ResponsePanel(); self.panel.resize(1100, 600); self.panel.show()
    def tearDown(self): self.panel.close(); self.panel.deleteLater(); self.app.processEvents()
    def response(self, body, headers=None):
        return dict(status=200, reason='OK', duration=1, size=len(body.encode()), body=body, headers=headers or {})
    def test_complete_copy_save_search_and_navigation_on_large_unicode_response(self):
        text = ('First page.\n'*20000)+'\nUnique needle: \U0001f680 end'
        self.panel.show_response(self.response(text)); self.app.processEvents()
        self.assertTrue(self.panel.pager.isVisible())
        self.assertLess(len(self.panel.editor.toPlainText()), len(text))
        self.panel.copy_response(); self.assertEqual(self.app.clipboard().text(), text)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)/'response.txt'
            with patch('ui_pages.QFileDialog.getSaveFileName', return_value=(str(path), '')): self.panel.save_response()
            self.assertEqual(path.read_text(encoding='utf-8'), text)
        self.panel.search.setText('Unique needle'); self.panel.find_next()
        self.assertGreater(self.panel._part, 0)
        self.assertEqual(self.panel.editor.textCursor().selectedText(), 'Unique needle')
        self.panel.previous_page.click(); self.assertEqual(self.panel._part, 0)
        self.panel.show_response(self.response('small')); self.assertFalse(self.panel.pager.isVisible())
        self.assertEqual(self.panel.editor.toPlainText(), 'small')
        self.assertIsNotNone(self.panel.editor.highlighter.document())
    def test_prepared_format_matches_preferences_and_preserves_raw_body(self):
        result = self.response('{"name":"value","items":[1,2]}', {'Content-Type':'application/json'})
        config = GeneralConfig(indent_count=2); self.panel.format_config = config
        prepare_response(result, False, config)
        with patch('response_format.format_body', side_effect=AssertionError('formatted again on GUI thread')):
            self.panel.show_response(result)
        self.assertEqual(json.loads(self.panel._copy_text), json.loads(result['body']))
        self.assertNotEqual(result['body'], self.panel._copy_text)
        self.assertEqual(self.panel._copy_text, result['display_body'])


if __name__ == '__main__': unittest.main()
