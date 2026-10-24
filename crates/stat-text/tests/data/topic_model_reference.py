"""Independent reference values for the latent Dirichlet allocation helpers.

For fixed corpora and fixed topic assignments z this computes, directly from
the definitions in Griffiths & Steyvers (2004), "Finding scientific topics",
PNAS 101 (suppl. 1), 5228-5235:

* log p(w | z)  (their eq. 2, the Dirichlet-multinomial marginal of the words
  given the assignments, with a symmetric topic-word prior beta):
      K [lnG(V beta) - V lnG(beta)]
        + sum_k [ sum_w lnG(n_kw + beta) - lnG(n_k + V beta) ]
* log p(z)  (the document-topic marginal with symmetric prior alpha):
      D [lnG(K alpha) - K lnG(alpha)]
        + sum_d [ sum_k lnG(n_dk + alpha) - lnG(n_d + K alpha) ]
* phi_kw = (n_kw + beta) / (n_k + V beta)
* theta_dk = (n_dk + alpha) / (n_d + K alpha)

Counts are formed with numpy (np.add.at) and the log-gamma function is
scipy.special.gammaln, so nothing is shared with the Rust implementation.

Run:  python3 topic_model_reference.py > topic_model_reference.json
"""

import json

import numpy as np
from scipy.special import gammaln


def reference(documents, z, n_terms, topics, alpha, beta):
    D, V, K = len(documents), n_terms, topics
    n_kw = np.zeros((K, V))
    n_dk = np.zeros((D, K))
    for d, (doc, zd) in enumerate(zip(documents, z)):
        np.add.at(n_kw, (np.asarray(zd, dtype=int), np.asarray(doc, dtype=int)), 1.0)
        np.add.at(n_dk[d], np.asarray(zd, dtype=int), 1.0)
    n_k = n_kw.sum(axis=1)
    n_d = n_dk.sum(axis=1)
    log_w_given_z = K * (gammaln(V * beta) - V * gammaln(beta)) + np.sum(
        gammaln(n_kw + beta).sum(axis=1) - gammaln(n_k + V * beta)
    )
    log_z = D * (gammaln(K * alpha) - K * gammaln(alpha)) + np.sum(
        gammaln(n_dk + alpha).sum(axis=1) - gammaln(n_d + K * alpha)
    )
    phi = (n_kw + beta) / (n_k[:, None] + V * beta)
    theta = (n_dk + alpha) / (n_d[:, None] + K * alpha)
    return {
        "documents": documents,
        "assignments": z,
        "n_terms": V,
        "topics": K,
        "alpha": alpha,
        "beta": beta,
        "log_likelihood": float(log_w_given_z),
        "log_prior": float(log_z),
        "phi": phi.tolist(),
        "theta": theta.tolist(),
    }


def main():
    cases = []
    # A hand-written corpus with an empty document and an unused term (5).
    docs = [[0, 1, 1, 2], [3, 4, 3, 0, 2], [], [1, 6, 6, 6]]
    z = [[0, 0, 1, 0], [2, 2, 2, 1, 0], [], [0, 1, 1, 2]]
    cases.append(reference(docs, z, 7, 3, 0.7, 0.3))
    # A random corpus: 20 documents, 30 terms, 5 topics.
    rng = np.random.default_rng(20040406)
    docs, z = [], []
    for _ in range(20):
        n = int(rng.integers(0, 40))
        docs.append([int(w) for w in rng.integers(0, 30, size=n)])
        z.append([int(k) for k in rng.integers(0, 5, size=n)])
    cases.append(reference(docs, z, 30, 5, 50.0 / 5, 0.1))
    # The same corpus with small priors.
    cases.append(reference(docs, z, 30, 5, 0.05, 0.01))
    print(json.dumps({"cases": cases}, indent=1))


if __name__ == "__main__":
    main()
