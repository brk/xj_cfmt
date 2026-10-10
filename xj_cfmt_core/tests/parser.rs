use xj_cfmt_core::*;

#[test]
fn stars_lengths_and_nul_boundaries() {
    let p = Plan::new(b"\xff%*.*hhd/%%/%ln\0%q").unwrap();
    assert_eq!(
        p.requirements,
        [
            Requirement::Int,
            Requirement::Int,
            Requirement::Int,
            Requirement::Count(Length::L)
        ]
    );
    assert!(p.count_stores);
    let Piece::Conversion(c) = p.pieces[1] else {
        panic!()
    };
    assert_eq!(
        (
            c.width,
            c.precision,
            c.value_argument,
            c.length,
            c.source_offset
        ),
        (
            Count::Argument(0),
            Count::Argument(1),
            Some(2),
            Length::Hh,
            1
        )
    );
    assert_eq!(p.pieces[0], Piece::Literal(b"\xff"));
}
#[test]
fn preserves_precision_and_all_lengths() {
    let p = Plan::new(b"%d %.d %hhd %hd %ld %lld %jd %zd %td %lf").unwrap();
    let conversions: Vec<_> = p
        .pieces
        .iter()
        .filter_map(|p| {
            if let Piece::Conversion(c) = p {
                Some(c)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(conversions[0].precision, Count::Absent);
    assert_eq!(conversions[1].precision, Count::Constant(0));
    assert_eq!(
        conversions.iter().map(|c| c.length).collect::<Vec<_>>(),
        [
            Length::None,
            Length::None,
            Length::Hh,
            Length::H,
            Length::L,
            Length::Ll,
            Length::J,
            Length::Z,
            Length::T,
            Length::L
        ]
    );
    let Piece::Conversion(c) = parse(b"%Lf").next().unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(c.length, Length::BigL);
    assert_eq!(Plan::new(b"%Lf").unwrap_err().kind, ErrorKind::Unsupported);
}
#[test]
fn structured_failures_and_explicit_unsupported_features() {
    for (bytes, kind, offset) in [
        (b"x%".as_slice(), ErrorKind::Incomplete, 1),
        (b"%q", ErrorKind::UnknownConversion, 1),
        (b"%1$d", ErrorKind::Positional, 2),
        (b"%*2$d", ErrorKind::Positional, 3),
        (b"%lls", ErrorKind::InvalidLength, 0),
        (b"%1%", ErrorKind::InvalidPercent, 0),
        (b"%2147483648d", ErrorKind::CountOverflow, 1),
        (b"%ls", ErrorKind::Unsupported, 0),
        (b"%m", ErrorKind::Unsupported, 0),
        (b"%'d", ErrorKind::Unsupported, 0),
        (b"%5n", ErrorKind::Unsupported, 0),
    ] {
        assert_eq!(
            Plan::new(bytes).unwrap_err(),
            Error { offset, kind },
            "{bytes:?}"
        );
    }
}
