import unittest
from mcp_context_benchmark import expand, stable


class DecoderTests(unittest.TestCase):
    def test_nested_and_escaped_pointers(self):
        original = {'result': {'a/~': [{'columns': ['x'], 'rows': [[None], [False]]}]},
                    'encoding': 'omen.tables.v1', 'table_paths': ['/a~1~0/0']}
        self.assertEqual(expand(original), {'result': {'a/~': [[{'x': None}, {'x': False}]]}})
        self.assertIn('encoding', original)

    def test_root_array(self):
        self.assertEqual(expand({'result': {'columns': ['x'], 'rows': [[1]]},
                                 'encoding': 'omen.tables.v1', 'table_paths': ['']}),
                         {'result': [{'x': 1}]})

    def test_ordinary_table_shaped_object_unchanged(self):
        value = {'result': {'columns': ['x'], 'rows': [[1]]}}
        self.assertEqual(expand(value), value)

    def test_invalid_row_rejected(self):
        with self.assertRaises(ValueError):
            expand({'result': {'columns': ['x'], 'rows': [[1, 2]]},
                    'encoding': 'omen.tables.v1', 'table_paths': ['']})

    def test_only_timestamp_ignored(self):
        self.assertEqual(stable({'result': {'generated_at': 'now', 'line': 3}}),
                         {'result': {'line': 3}})


if __name__ == '__main__':
    unittest.main()
