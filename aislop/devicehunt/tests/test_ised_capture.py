import unittest

from research.capture_ised import Form


class IsedFormTests(unittest.TestCase):
    def test_only_active_public_form_defaults_and_clicked_submit_are_sent(self):
        f = Form('''<form action="https://www.canada.ca/search"><input name="q" value="wrong"></form>
          <form action="/equipmentSearch/searchRadioEquipments">
          <input type="hidden" name="execution" value="e1s4">
          <input name="companyName" value="Amazon Data Services">
          <select name="sortCriteria"><option value="HVIN">HVIN</option>
          <option value="CN" selected>CN</option></select>
          <input type="checkbox" name="off"><input type="checkbox" name="grouping" checked>
          <input type="submit" name="_eventId_refresh" value="Update">
          <input type="submit" name="_eventId_cancel" value="Cancel"></form>''')
        self.assertEqual(f.values, {'execution': 'e1s4', 'companyName': 'Amazon Data Services',
                                    'sortCriteria': 'CN', 'grouping': 'on'})
        self.assertEqual(f.buttons, {'_eventId_refresh': 'Update', '_eventId_cancel': 'Cancel'})

    def test_export_preparation_and_file_links_are_distinct(self):
        f = Form('''<a id="downloadButton" href="?execution=e1s6&amp;_eventId=download">CSV</a>
          <a id="downloadCSVButton" href="?execution=e1s6&amp;_eventId=downloadCSV"></a>''')
        self.assertEqual(f.download, '?execution=e1s6&_eventId=download')
        self.assertEqual(f.download_csv, '?execution=e1s6&_eventId=downloadCSV')

    def test_pagination_uses_displayed_next_link(self):
        f = Form('<a href="?offset=20&amp;execution=e1s4" rel="next">Next</a>')
        self.assertEqual(f.next_page, '?offset=20&execution=e1s4')
