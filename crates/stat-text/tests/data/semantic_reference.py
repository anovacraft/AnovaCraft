"""Reference values for the document-term matrix, TF-IDF weighting and
latent semantic analysis tests (crates/stat-text/tests/semantic_reference.rs).

Independent of the Rust code: words are split on spaces (the corpus has no
punctuation, capitals or stop words), the vocabulary is ordered by
decreasing total count then alphabetically, TF-IDF is tf * ln(N / df) with
unit-length rows, and the SVD is numpy.linalg.svd. Signs are fixed so that
each dimension's largest |loading| is positive.

Run: python3 semantic_reference.py > semantic_reference.json
"""
import json
import numpy as np

docs = [
    "espresso crema bitter espresso roast",
    "grinder burr noise grinder loud",
    "milk frother steam milk foam",
    "espresso roast beans crema",
    "loud noise grinder motor",
    "steam wand milk foam latte",
    "beans roast bitter flavour",
    "burr grinder motor noise",
    "latte foam milk frother",
    "flavour espresso beans roast crema",
    "descaling water tank limescale",
    "water tank leaks descaling",
    "limescale descaling water filter",
    "filter water tank espresso",
]
counts = {}
df = {}
for d in docs:
    ws = d.split()
    for w in ws:
        counts[w] = counts.get(w, 0) + 1
    for w in set(ws):
        df[w] = df.get(w, 0) + 1
vocab = sorted(counts, key=lambda w: (-counts[w], w))
n = len(docs)
tf = np.zeros((n, len(vocab)))
for i, d in enumerate(docs):
    for w in d.split():
        tf[i, vocab.index(w)] += 1
idf = np.log(n / np.array([df[w] for w in vocab]))
a = tf * idf
a = a / np.linalg.norm(a, axis=1, keepdims=True)
u, s, vt = np.linalg.svd(a, full_matrices=False)
k = 3
v = vt[:k].T.copy()
scores = (u[:, :k] * s[:k]).copy()
for i in range(k):
    j = np.argmax(np.abs(v[:, i]))
    if v[j, i] < 0:
        v[:, i] *= -1
        scores[:, i] *= -1
print(json.dumps({
    "documents": docs,
    "terms": vocab,
    "counts": tf.tolist(),
    "tfidf_normalized": a.tolist(),
    "singular_values": s.tolist(),
    "term_loadings": v.tolist(),
    "document_scores": scores.tolist(),
    "total_sum_squares": float((a ** 2).sum()),
}, indent=1))
