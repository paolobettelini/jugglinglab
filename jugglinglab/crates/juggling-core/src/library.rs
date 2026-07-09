use crate::jml::{PatternRecord, parse_jml};

const BUILTIN_JML_FILES: &[(&str, &str)] = &[
    //(
    //    "basic_siteswaps.jml",
    //    include_str!("../../../../patterns/basic_siteswaps.jml"),
    //),
    //(
    //    "basic_solo.jml",
    //    include_str!("../../../../patterns/basic_solo.jml"),
    //),
    //(
    //    "basic_passing.jml",
    //    include_str!("../../../../patterns/basic_passing.jml"),
    //),
    //(
    //    "basic_how to.jml",
    //    include_str!("../../../../patterns/basic_how to.jml"),
    //),
    //(
    //    "hss_2JugglersAsymmetric.jml",
    //    include_str!("../../../../patterns/hss_2JugglersAsymmetric.jml"),
    //),
    //(
    //    "hss_2JugglersSymmetric.jml",
    //    include_str!("../../../../patterns/hss_2JugglersSymmetric.jml"),
    //),
    //(
    //    "hss_2UnequalPassers.jml",
    //    include_str!("../../../../patterns/hss_2UnequalPassers.jml"),
    //),
    //(
    //    "hss_3JugglersAsymmetric.jml",
    //    include_str!("../../../../patterns/hss_3JugglersAsymmetric.jml"),
    //),
    //(
    //    "hss_3JugglersSymmetric.jml",
    //    include_str!("../../../../patterns/hss_3JugglersSymmetric.jml"),
    //),
    //(
    //    "hss_PrechacWeaves.jml",
    //    include_str!("../../../../patterns/hss_PrechacWeaves.jml"),
    //),
    //(
    //    "hss_TwoHandedPatterns.jml",
    //    include_str!("../../../../patterns/hss_TwoHandedPatterns.jml"),
    //),
    //(
    //    "jboyce_Juggling Lab demo.jml",
    //    include_str!("../../../../patterns/jboyce_Juggling Lab demo.jml"),
    //),
    //(
    //    "Alanz_3BallBounce V 2Edit.jml",
    //    include_str!("../../../../patterns/Alanz_3BallBounce V 2Edit.jml"),
    //),
    //(
    //    "Alanz_Multiplex etcetera.jml",
    //    include_str!("../../../../patterns/Alanz_Multiplex etcetera.jml"),
    //),
    //(
    //    "Alanz_Some Patterns Without 3's.jml",
    //    include_str!("../../../../patterns/Alanz_Some Patterns Without 3's.jml"),
    //),
    //(
    //    "Alanz_Synchronous Favorites.jml",
    //    include_str!("../../../../patterns/Alanz_Synchronous Favorites.jml"),
    //),
    //(
    //    "Are you God.jml",
    //    include_str!("../../../../patterns/Are you God.jml"),
    //),
    //(
    //    "Omnikrabundi_FunWithJugglingLab.jml",
    //    include_str!("../../../../patterns/Omnikrabundi_FunWithJugglingLab.jml"),
    //),
    //(
    //    "arham_stupid jugging lab patterns.jml",
    //    include_str!("../../../../patterns/arham_stupid jugging lab patterns.jml"),
    //),
];

pub fn builtin_records() -> Vec<PatternRecord> {
    let mut records = Vec::new();

    for (filename, xml) in BUILTIN_JML_FILES {
        match parse_jml(xml) {
            Ok(library) => {
                if let Some(title) = library.title {
                    records.push(PatternRecord {
                        display: title,
                        notation: None,
                        config: None,
                        animprefs: None,
                        info: Some((*filename).to_string()),
                        tags: vec!["library".to_string()],
                        raw_pattern: None,
                    });
                }
                records.extend(library.records);
            }
            Err(err) => records.push(PatternRecord {
                display: format!("{filename}: {err}"),
                notation: None,
                config: None,
                animprefs: None,
                info: None,
                tags: vec!["error".to_string()],
                raw_pattern: None,
            }),
        }
    }

    if records.iter().all(|record| !record.is_playable()) {
        records.push(PatternRecord::siteswap("3 cascade", "pattern=3"));
    }

    records
}
