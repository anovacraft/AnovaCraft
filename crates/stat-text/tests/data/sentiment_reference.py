"""Independent reference values for stat-text's lexicon sentiment scores
(`crates/stat-text/src/sentiment.rs`).

Runs the published vaderSentiment package (Hutto & Gilbert 2014, MIT licence;
`python3 -m pip install vaderSentiment`, tested with 3.3.2) on sentences that
stay inside the part of VADER that stat-text implements: lexicon valences,
negation of the three preceding tokens by the scalar -0.74, the alpha = 15
normalisation, and VADER's pos/neg/neu proportions.  The sentences therefore
avoid VADER's other heuristics: no capitals (ALL-CAPS emphasis), no "!" or "?"
(punctuation emphasis), no booster or dampener words (very, really, so, quite,
more, most, kind of, little, almost, ...), no "but", no "least", no idioms or
special phrases ("the shit", "bus stop", ...), no emoticons or emoji, no
"never so/this" or "without doubt", no "this" or "so" directly before a
sentiment word, "no" only at distance 1 or 2 from the word it negates, and no "nobody"
(a negator in stat-text but not in VADER's list).
Words are separated by single spaces with no punctuation, so VADER's
whitespace split and Unicode word segmentation give the same tokens.

Besides VADER's rounded output (compound to 4 decimals, proportions to 3),
the script captures the unrounded per-token valences VADER sums, by wrapping
`SentimentIntensityAnalyzer.score_valence`, and records the raw sum s and the
unrounded compound and proportions computed with VADER's own `normalize` and
`_sift_sentiment_scores`.

Output: `sentiment_reference.tsv` next to this script, one row per sentence:
sentence, raw sum, compound, pos, neg, neu (unrounded), then VADER's rounded
compound, pos, neg, neu.  Run with `python3 -I sentiment_reference.py`.
"""
import math
import os

import vaderSentiment
from vaderSentiment.vaderSentiment import (
    SentimentIntensityAnalyzer,
    normalize,
)

SENTENCES = [
    # empty and sentiment-free text
    "",
    "the meeting is on tuesday",
    "we walked to the station and took the train",
    "xyzzy plugh quux",
    # single lexicon words
    "good",
    "bad",
    "the food was good",
    "the service was terrible",
    "i love the garden",
    "they hate waiting in line",
    "the weather today is wonderful",
    "the room smelled awful",
    "she is a beautiful dancer",
    "the plot was boring",
    "our team won the match",
    "the printer is broken again",
    # several sentiment words
    "the food was good and the staff were friendly",
    "the food was good and the service was terrible",
    "happy people make happy places",
    "sad and angry and tired",
    "excellent wonderful great",
    "the ugly car had a nice engine",
    "i hope the trip is fun",
    "the poor results made everyone sad",
    "we enjoy the calm evenings with good friends",
    "the failure caused panic and fear",
    "the cake was delicious and the coffee was fresh",
    "a horrible cruel and stupid decision",
    # negation at distance one
    "not good",
    "the food was not good",
    "the movie was not bad",
    "i do not love it",
    "he never lies",
    "i don't like the colour",
    "it isn't terrible",
    "the plan wasn't great",
    "we can't enjoy the show",
    "they cannot win",
    "nothing will help",
    "the answer won't help",
    "the test didn't fail",
    "she doesn't care",
    "i dont like waiting",
    "the parts are without damage",
    # negation at distance two and three
    "not a good idea",
    "it was not a nice day",
    "the ending is not at all happy",
    "i don't think it is bad",
    "we never had a great time",
    "we did not see a good show",
    "they didn't find the hotel comfortable",
    "none of them were good",
    "none were good",
    # double and overlapping negation
    "not not good",
    "it isn't not funny",
    "neither good nor bad",
    "i don't hate it and i don't love it",
    "not good not bad",
    "the staff weren't rude and weren't helpful",
    "never not happy",
    # no as negator and as a word
    "no problem",
    "no good deed",
    "there is no hope",
    "no thanks",
    "we have no complaints at all",
    # outside the lexicon and repeated tokens
    "the zebra crossed the road",
    "good good good",
    "bad bad bad bad",
    "a b c d e f good",
]


class CapturingAnalyzer(SentimentIntensityAnalyzer):
    """VADER with score_valence wrapped to keep the unrounded valences."""

    def __init__(self):
        super().__init__()
        self.last = None

    def score_valence(self, sentiments, text):
        self.last = list(sentiments)
        return super().score_valence(sentiments, text)


def unrounded(sentiments):
    """VADER's score_valence without rounding (no punctuation emphasis)."""
    if not sentiments:
        return 0.0, 0.0, 0.0, 0.0, 0.0
    s = float(sum(sentiments))
    compound = normalize(s)
    pos_sum, neg_sum, neu_count = SentimentIntensityAnalyzer._sift_sentiment_scores(sentiments)
    total = pos_sum + math.fabs(neg_sum) + neu_count
    return (
        s,
        compound,
        math.fabs(pos_sum / total),
        math.fabs(neg_sum / total),
        math.fabs(neu_count / total),
    )


def main():
    assert all(s == s.lower() and "!" not in s and "?" not in s for s in SENTENCES)
    analyzer = CapturingAnalyzer()
    rows = []
    for sentence in SENTENCES:
        rounded = analyzer.polarity_scores(sentence)
        s, compound, pos, neg, neu = unrounded(analyzer.last)
        assert round(compound, 4) == rounded["compound"], sentence
        rows.append(
            "\t".join(
                [sentence]
                + [repr(x) for x in (s, compound, pos, neg, neu)]
                + [repr(rounded[k]) for k in ("compound", "pos", "neg", "neu")]
            )
        )
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sentiment_reference.tsv")
    with open(path, "w", encoding="utf-8") as out:
        out.write(
            "# vaderSentiment %s; columns: sentence, sum, compound, pos, neg, neu, "
            "vader_compound, vader_pos, vader_neg, vader_neu\n" % getattr(vaderSentiment, "__version__", "3.3.2")
        )
        out.write("\n".join(rows) + "\n")
    print("wrote %d sentences to %s" % (len(rows), path))


if __name__ == "__main__":
    main()
