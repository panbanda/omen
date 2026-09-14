import unittest
from quality_benchmark import gate, reliable, score


class QualityTests(unittest.TestCase):
    def test_wrong_target_is_false_and_missed(self):
        self.assertEqual(score(['wrong'], ['right']),
                         dict(tp=0, fp=1, fn=1, precision=0, recall=0))

    def test_empty_precision_is_not_perfect(self):
        self.assertIsNone(score([], ['right'])['precision'])

    def test_duplicates_do_not_inflate(self):
        self.assertEqual(score(['a', 'a'], ['a'])['tp'], 1)

    def test_gain_cannot_hide_recall_loss(self):
        self.assertFalse(gate(dict(fp=4, fn=0), dict(fp=0, fn=1), True))

    def test_errors_including_warmups_fail(self):
        self.assertFalse(reliable([dict(error='timeout'), dict(error=None, report_sha256='a')]))

    def test_changing_output_fails(self):
        self.assertFalse(reliable([dict(error=None, report_sha256='a'), dict(error=None, report_sha256='b')]))


if __name__ == '__main__':
    unittest.main()
