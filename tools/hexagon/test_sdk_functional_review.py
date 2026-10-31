"""Hosted model-free controls for quoted text, directive adjacency and notices."""

import codecs
import unittest

from sdk_functional_review import compare_functional, decoded_text, functional_metadata, functional_tokens, legal_review


LIMITS = {"functionalTokens": 10000, "functionalDeclarations": 64,
          "declarationTokens": 256, "preprocessorDirectives": 64}


class FunctionalReviewTests(unittest.TestCase):
    def tokens(self, text):
        return functional_tokens(text, 10000)[0]

    def test_comments_do_not_consume_literal_or_include_contents(self):
        text = '#include <api // header.h>\nconst char *remote_url = "https://x/*literal*/"; /* tail */\n'
        tokens = self.tokens(text)
        self.assertIn('<api // header.h>', tokens)
        self.assertIn('"https://x/*literal*/"', tokens)
        self.assertNotIn("tail", tokens)
        self.assertEqual(self.tokens("int remote_x /* comment */ ;"), ["int", "remote_x", ";"])

    def test_macro_function_adjacency_and_directive_ends_remain_distinct(self):
        function = self.tokens("#define FN(x) x\n")
        object_like = self.tokens("#define FN (x) x\n")
        comment_gap = self.tokens("#define FN/**/(x) x\n")
        self.assertIn("<PP-MACRO-FUNCTIONLIKE>", function)
        self.assertIn("<PP-MACRO-OBJECTLIKE-PAREN>", object_like)
        self.assertEqual(object_like, comment_gap)
        self.assertNotEqual(function, object_like)
        self.assertNotEqual(self.tokens("#define X 1\nY"), self.tokens("#define X 1 Y\n"))

    def test_line_splices_precede_comment_recognition(self):
        self.assertEqual(self.tokens("#define FN(x) ((x) \\\n+ 1)\n"), self.tokens("#define FN(x) ((x) + 1)\n"))
        self.assertEqual(self.tokens("// comment\\\nint remote_x;\n"), [])

    def test_literal_prefix_adjacency_is_not_whitespace(self):
        self.assertEqual(self.tokens('L"x"'), ['L"x"'])
        self.assertNotEqual(self.tokens('L"x"'), self.tokens('L "x"'))

    def test_typedef_callback_enum_and_method_candidates_preserve_tokens(self):
        data = (b'typedef void (*dspqueue_callback)(int fd, void *ctx);\n'
                b'enum dspqueue_flags { DSPQUEUE_BUFFER_REF=4, DSPQUEUE_BUFFER_DEREF=8 };\n'
                b'int dspqueue_read(int fd, unsigned timeout);\n')
        meta = functional_metadata(data, LIMITS)
        declarations = meta["declarationCandidates"]["declarations"]
        self.assertEqual(len(declarations), 3)
        self.assertEqual(declarations[0]["callbackTypedefNameCandidates"], ["dspqueue_callback"])
        self.assertTrue(declarations[1]["enumPresent"])
        self.assertIn("DSPQUEUE_BUFFER_REF", declarations[1]["tokens"])
        comparison = compare_functional(b"int remote_call(int arg);", b"short remote_call(int arg);", LIMITS)
        self.assertFalse(comparison["normalizedTokenStreamsEqual"])
        self.assertFalse(comparison["matchingNamesOrTokensProveABIEquivalence"])
        self.assertFalse(comparison["callbackLifetimeQualified"])

    def test_unknown_source_syntax_preserves_pending_state(self):
        meta = functional_metadata(b"int @remote_value;", LIMITS)
        self.assertEqual(meta["status"], "pending-functional-token-review")
        self.assertNotIn("normalizedTokenSha256", meta)
        comparison = compare_functional(b"int @remote_value;", b"int remote_value;", LIMITS)
        self.assertIsNone(comparison["normalizedTokenStreamsEqual"])
        self.assertFalse(comparison["deviceInvocationAuthorized"])

    def test_known_boms_and_uncertain_windows_text_fallback(self):
        for bom, encoding in ((codecs.BOM_UTF16_LE, "utf-16-le"), (codecs.BOM_UTF32_LE, "utf-32-le")):
            text, observation = decoded_text(bom + "QAIC terms".encode(encoding))
            self.assertEqual(text, "QAIC terms")
            self.assertEqual(observation["basis"], "explicit-BOM")
            self.assertFalse(observation["uncertain"])
        review = legal_review(b"\x93QAIC\x94 product license agreement", 25)
        self.assertEqual(review["encodingObservation"]["encoding"], "cp1252")
        self.assertTrue(review["encodingObservation"]["uncertain"])
        self.assertEqual(review["termOccurrences"]["qaic-name"]["count"], 1)
        self.assertFalse(review["keywordMatchesEstablishGrant"])
        self.assertFalse(review["sdkToolTermsQualified"])
        self.assertLessEqual(len(review["necessaryReviewExcerpt"]["text"].split()), 25)


if __name__ == "__main__":
    unittest.main()
