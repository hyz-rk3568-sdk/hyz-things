import unittest

from bank import render_paper, parse_paper
from verify import validate_document
from test_importer import SAMPLE


class VerificationTests(unittest.TestCase):
    def test_dangling_material_is_rejected(self):
        text = render_paper(parse_paper(SAMPLE)).replace(
            "materials: []", "materials: [M9]"
        )
        with self.assertRaisesRegex(ValueError, "material"):
            validate_document(text)


if __name__ == "__main__":
    unittest.main()
