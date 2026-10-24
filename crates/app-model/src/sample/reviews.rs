//! Sample for text analysis: simulated product reviews.

use super::{Sample, col, table};
use crate::project::SavedReport;
use crate::{SentimentRequest, TextExplorationRequest, TopicModelRequest};
use stat_core::rng::Rng;
use stat_data::{Column, DataTable, MeasurementLevel};

/// The review themes: name, praising sentences, complaining sentences.
const THEMES: [(&str, &[&str], &[&str]); 6] = [
    (
        "Coffee taste",
        &[
            "The espresso tastes rich and smooth with a lovely crema.",
            "Great coffee every morning, full flavour and never bitter.",
            "The shots are strong and balanced, better than our local cafe.",
            "Wonderful aroma and a thick crema on every espresso.",
            "Even cheap beans make a delicious, smooth cup.",
        ],
        &[
            "The espresso tastes bitter and burnt whatever beans we use.",
            "Weak, watery coffee with hardly any crema.",
            "The shots are sour and thin, not what I expected for the price.",
            "No matter the grind the coffee tastes flat and bitter.",
            "Disappointing flavour, the espresso is lukewarm and weak.",
        ],
    ),
    (
        "Milk frothing",
        &[
            "The steam wand makes silky milk foam for a perfect latte.",
            "Frothing milk is easy and the cappuccino foam is lovely.",
            "Steam pressure is excellent, the milk is ready in seconds.",
            "We love the latte art we can pour with the frother.",
        ],
        &[
            "The steam wand is weak and the milk foam collapses.",
            "Frothing milk takes forever and the foam is bubbly and thin.",
            "The frother sputters and splashes hot milk everywhere.",
            "Not happy with the steam wand, it barely warms the milk.",
        ],
    ),
    (
        "Cleaning and descaling",
        &[
            "Cleaning is quick, the drip tray and brew group come out easily.",
            "The descaling programme is simple and the water tank is easy to fill.",
            "Easy to keep clean and the water filter is a nice touch.",
            "Daily cleaning takes a minute, which we really appreciate.",
        ],
        &[
            "Descaling is a messy chore and the reminder light never goes off.",
            "The water tank leaks onto the counter after every fill.",
            "Limescale builds up fast and cleaning the brew group is awkward.",
            "The drip tray overflows and the water filter is hard to replace.",
        ],
    ),
    (
        "Noise and grinder",
        &[
            "The built-in grinder is quiet and the burr grind is consistent.",
            "Surprisingly quiet machine, the grinder does not wake the house.",
            "The burr grinder has plenty of settings and grinds evenly.",
        ],
        &[
            "The grinder is incredibly loud and the noise wakes everyone.",
            "The burr grinder jams and the motor makes a terrible noise.",
            "Loud rattling noise from the pump and the grinder is uneven.",
        ],
    ),
    (
        "Delivery and packaging",
        &[
            "Delivery was fast and the box was well packed.",
            "It arrived early, carefully packaged with every part included.",
            "Quick shipping and the packaging protected it perfectly.",
        ],
        &[
            "Delivery was late and the box arrived damaged.",
            "The package was crushed and the drip tray was cracked on arrival.",
            "Shipping took three weeks and parts were missing from the box.",
        ],
    ),
    (
        "Customer service",
        &[
            "Customer service answered quickly and replaced a part under warranty.",
            "Support was friendly and helpful when we had a question.",
            "The warranty claim was handled fast, excellent service.",
        ],
        &[
            "Customer service never replied to my emails about the warranty.",
            "Support was rude and refused to honour the warranty.",
            "Waited weeks for a repair, terrible customer service.",
        ],
    ),
];

/// Sentences that say nothing about a theme.
const FILLER: [&str; 8] = [
    "We bought it last month.",
    "We use it every morning before work.",
    "It replaced an old pod machine.",
    "I make about three drinks a day.",
    "It was a birthday present.",
    "It sits next to the kettle in our small kitchen.",
    "My partner mostly drinks flat whites.",
    "We have had it for about six weeks.",
];

/// Machine models and their mean ratings.
const MODELS: [(&str, f64); 3] = [("Compact", 3.2), ("Deluxe", 3.7), ("Barista Pro", 4.1)];

/// Theme shares (in `THEMES` order).
const THEME_SHARES: [f64; 6] = [0.27, 0.18, 0.17, 0.14, 0.13, 0.11];

fn draw(rng: &mut Rng, probabilities: &[f64]) -> usize {
    let u = rng.next_f64() * probabilities.iter().sum::<f64>();
    let mut cum = 0.0;
    for (i, p) in probabilities.iter().enumerate() {
        cum += p;
        if u < cum {
            return i;
        }
    }
    probabilities.len() - 1
}

fn pick<'a>(rng: &mut Rng, items: &[&'a str]) -> &'a str {
    items[rng.below(items.len())]
}

/// **Simulated** reviews of 300 espresso machines of three models. Each
/// review has a main theme (coffee taste 27%, milk frothing 18%, cleaning
/// and descaling 17%, noise and grinder 14%, delivery and packaging 13%,
/// customer service 11%) with one or two sentences about it, a second theme
/// in 40% of reviews, and a neutral sentence in 60%. The star rating is
/// drawn around the model's mean (Compact 3.2, Deluxe 3.7, Barista Pro 4.1;
/// normal with SD 1.1, rounded and kept in 1–5), and each sentence praises
/// with probability (rating − 1) / 4, otherwise complains. Sentences come
/// from short templates written for this sample. Drawn with the SplitMix64
/// generator seeded 0x7E47; the main theme is kept in a column to compare
/// with the topics and clusters found.
#[must_use]
pub fn espresso_reviews() -> DataTable {
    let mut rng = Rng::new(0x7E47);
    let n = 300;
    let (mut model, mut rating, mut text, mut theme) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..n {
        let m = rng.below(MODELS.len());
        let stars = (MODELS[m].1 + 1.1 * rng.normal()).round().clamp(1.0, 5.0);
        let praise = (stars - 1.0) / 4.0;
        let sentence = |rng: &mut Rng, t: usize| {
            let (_, good, bad) = THEMES[t];
            if rng.next_f64() < praise { pick(rng, good) } else { pick(rng, bad) }
        };
        let main = draw(&mut rng, &THEME_SHARES);
        let mut parts = vec![sentence(&mut rng, main)];
        if rng.next_f64() < 0.5 {
            let s = sentence(&mut rng, main);
            if s != parts[0] {
                parts.push(s);
            }
        }
        if rng.next_f64() < 0.4 {
            let mut other = draw(&mut rng, &THEME_SHARES);
            if other == main {
                other = (other + 1) % THEMES.len();
            }
            parts.push(sentence(&mut rng, other));
        }
        if rng.next_f64() < 0.6 {
            let at = rng.below(parts.len() + 1);
            parts.insert(at, pick(&mut rng, &FILLER));
        }
        model.push(MODELS[m].0.to_owned());
        rating.push(stars);
        text.push(parts.join(" "));
        theme.push(THEMES[main].0.to_owned());
    }
    let mut id = Column::numeric("Review", (1..=n).map(|i| i as f64).collect());
    id.level = MeasurementLevel::Nominal;
    let mut stars = Column::numeric("Rating", rating);
    stars.level = MeasurementLevel::Ordinal;
    table(
        "Espresso machine reviews (simulated)",
        vec![
            id,
            Column::text("Model", model),
            stars,
            Column::text("Review text", text),
            Column::text("Simulated main theme", theme),
        ],
    )
}

pub(crate) fn espresso_reviews_sample() -> Sample {
    let t = espresso_reviews();
    let c = col(&t, "Review text");
    let explore = TextExplorationRequest { dimensions: 6, clusters: 6, ..TextExplorationRequest::new(c) };
    let topics = TopicModelRequest { topics: 6, ..TopicModelRequest::new(c) };
    Sample {
        reports: vec![
            SavedReport::TextExploration { request: explore },
            SavedReport::TopicModel { request: topics },
            SavedReport::Sentiment { request: SentimentRequest::new(c) },
        ],
        table: t,
    }
}
