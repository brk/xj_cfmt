use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use std::collections::HashMap;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, Ident, Lit, Path, Result, Token, parse_macro_input};

#[proc_macro]
pub fn cprintf_impl(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as PlainInput), Kind::Printf)
}

#[proc_macro]
pub fn ceprintf_impl(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as PlainInput), Kind::Eprintf)
}

#[proc_macro]
pub fn cemit_impl(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as DestinationInput), Kind::Emit)
}

#[proc_macro]
pub fn cwrite_impl(input: TokenStream) -> TokenStream {
    expand(parse_macro_input!(input as DestinationInput), Kind::Write)
}

#[proc_macro]
pub fn csnprintf_impl(input: TokenStream) -> TokenStream {
    expand(
        parse_macro_input!(input as DestinationInput),
        Kind::Snprintf,
    )
}

enum Kind {
    Printf,
    Eprintf,
    Emit,
    Write,
    Snprintf,
}

struct PlainInput {
    crate_path: Path,
    format: Lit,
    args: Vec<InvocationArg>,
}

struct DestinationInput {
    crate_path: Path,
    destination: Expr,
    format: Lit,
    args: Vec<InvocationArg>,
}

struct InvocationArg {
    name: Option<Ident>,
    expression: Expr,
}

impl Parse for InvocationArg {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let fork = input.fork();
        if fork.peek(Ident) {
            let name: Ident = fork.parse()?;
            if fork.peek(Token![=]) {
                let _: Ident = input.parse()?;
                input.parse::<Token![=]>()?;
                return Ok(Self {
                    name: Some(name),
                    expression: input.parse()?,
                });
            }
        }
        Ok(Self {
            name: None,
            expression: input.parse()?,
        })
    }
}

fn parse_tail(input: ParseStream<'_>) -> Result<(Lit, Vec<InvocationArg>)> {
    let format: Lit = input.parse()?;
    let args = if input.is_empty() {
        Vec::new()
    } else {
        input.parse::<Token![,]>()?;
        Punctuated::<InvocationArg, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect()
    };
    Ok((format, args))
}

impl Parse for PlainInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let crate_path = input.parse()?;
        input.parse::<Token![;]>()?;
        let (format, args) = parse_tail(input)?;
        Ok(Self {
            crate_path,
            format,
            args,
        })
    }
}

impl Parse for DestinationInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let crate_path = input.parse()?;
        input.parse::<Token![;]>()?;
        let destination = input.parse()?;
        input.parse::<Token![;]>()?;
        let (format, args) = parse_tail(input)?;
        Ok(Self {
            crate_path,
            destination,
            format,
            args,
        })
    }
}

impl From<PlainInput> for CommonInput {
    fn from(value: PlainInput) -> Self {
        Self {
            crate_path: value.crate_path,
            destination: None,
            format: value.format,
            args: value.args,
        }
    }
}

impl From<DestinationInput> for CommonInput {
    fn from(value: DestinationInput) -> Self {
        Self {
            crate_path: value.crate_path,
            destination: Some(value.destination),
            format: value.format,
            args: value.args,
        }
    }
}

struct CommonInput {
    crate_path: Path,
    destination: Option<Expr>,
    format: Lit,
    args: Vec<InvocationArg>,
}

trait IntoCommon {
    fn into_common(self) -> CommonInput;
}
impl IntoCommon for PlainInput {
    fn into_common(self) -> CommonInput {
        self.into()
    }
}
impl IntoCommon for DestinationInput {
    fn into_common(self) -> CommonInput {
        self.into()
    }
}

fn expand(input: impl IntoCommon, kind: Kind) -> TokenStream {
    match expand_inner(input.into_common(), kind) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

#[derive(Clone)]
enum ArgRef {
    Index(usize),
    Name(String),
}

#[derive(Clone)]
enum CountSpec {
    None,
    Literal(usize),
    Argument(ArgRef),
    Star,
}

#[derive(Clone, Copy)]
enum Alignment {
    Unspecified,
    Left,
    Right,
    Center,
}

#[derive(Clone)]
struct FormatSpec {
    present: bool,
    fill: u8,
    align: Alignment,
    sign: Option<u8>,
    alternate: bool,
    zero: bool,
    width: CountSpec,
    precision: CountSpec,
    ty: String,
}

#[derive(Clone)]
struct Placeholder {
    argument: Option<ArgRef>,
    spec: FormatSpec,
}

enum Segment {
    Literal(Vec<u8>),
    Placeholder(Placeholder),
}

fn format_bytes(literal: &Lit) -> Result<(Vec<u8>, Span, bool)> {
    match literal {
        Lit::Str(value) => Ok((value.value().into_bytes(), value.span(), false)),
        Lit::ByteStr(value) => Ok((value.value(), value.span(), true)),
        _ => Err(syn::Error::new_spanned(
            literal,
            "format argument must be a string or byte-string literal",
        )),
    }
}

fn parse_format(literal: &Lit) -> Result<Vec<Segment>> {
    let (bytes, span, is_byte_string) = format_bytes(literal)?;
    let mut segments = Vec::new();
    let mut text = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' if bytes.get(index + 1) == Some(&b'{') => {
                text.push(b'{');
                index += 2;
            }
            b'}' if bytes.get(index + 1) == Some(&b'}') => {
                text.push(b'}');
                index += 2;
            }
            b'{' => {
                if !text.is_empty() {
                    segments.push(Segment::Literal(std::mem::take(&mut text)));
                }
                let start = index + 1;
                let relative_end = bytes[start..]
                    .iter()
                    .position(|&byte| byte == b'}')
                    .ok_or_else(|| syn::Error::new(span, "unmatched `{` in format string"))?;
                let end = start + relative_end;
                if bytes[start..end].contains(&b'{') {
                    return Err(syn::Error::new(span, "nested `{` in format placeholder"));
                }
                if is_byte_string && !bytes[start..end].is_ascii() {
                    return Err(syn::Error::new(
                        span,
                        "byte-string format placeholders, including fill, must be ASCII",
                    ));
                }
                segments.push(Segment::Placeholder(parse_placeholder(
                    &bytes[start..end],
                    span,
                )?));
                index = end + 1;
            }
            b'}' => return Err(syn::Error::new(span, "unmatched `}` in format string")),
            byte => {
                text.push(byte);
                index += 1;
            }
        }
    }
    if !text.is_empty() {
        segments.push(Segment::Literal(text));
    }
    Ok(segments)
}

fn parse_arg_ref(bytes: &[u8], span: Span) -> Result<Option<ArgRef>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let text = std::str::from_utf8(bytes).expect("placeholder was checked as ASCII");
    if bytes.iter().all(u8::is_ascii_digit) {
        return text
            .parse()
            .map(ArgRef::Index)
            .map(Some)
            .map_err(|_| syn::Error::new(span, "argument index is too large"));
    }
    syn::parse_str::<Ident>(text)
        .map(|_| Some(ArgRef::Name(text.to_owned())))
        .map_err(|_| syn::Error::new(span, "invalid format argument reference"))
}

fn parse_placeholder(bytes: &[u8], span: Span) -> Result<Placeholder> {
    let colon = bytes.iter().position(|&byte| byte == b':');
    let (argument_bytes, spec_bytes) = match colon {
        Some(index) => (&bytes[..index], Some(&bytes[index + 1..])),
        None => (bytes, None),
    };
    let argument = parse_arg_ref(argument_bytes, span)?;
    let spec = parse_spec(spec_bytes, span)?;
    Ok(Placeholder { argument, spec })
}

fn parse_spec(bytes: Option<&[u8]>, span: Span) -> Result<FormatSpec> {
    let Some(bytes) = bytes else {
        return Ok(FormatSpec {
            present: false,
            fill: b' ',
            align: Alignment::Unspecified,
            sign: None,
            alternate: false,
            zero: false,
            width: CountSpec::None,
            precision: CountSpec::None,
            ty: String::new(),
        });
    };
    let text = std::str::from_utf8(bytes).expect("string literal placeholders are UTF-8");
    let mut at = 0;
    let mut fill = b' ';
    let mut align = Alignment::Unspecified;
    let to_align = |byte| match byte {
        b'<' => Some(Alignment::Left),
        b'>' => Some(Alignment::Right),
        b'^' => Some(Alignment::Center),
        _ => None,
    };
    let mut characters = text.char_indices();
    if let Some((_, first)) = characters.next() {
        if let Some((second_at, second)) = characters.next() {
            if let Some(value) = second.is_ascii().then(|| to_align(second as u8)).flatten() {
                if !first.is_ascii() {
                    return Err(syn::Error::new(span, "format fill must be ASCII"));
                }
                if matches!(first, '{' | '}') {
                    return Err(syn::Error::new(
                        span,
                        "braces cannot be format fill characters",
                    ));
                }
                fill = first as u8;
                align = value;
                at = second_at + second.len_utf8();
            }
        }
        if at == 0 && first.is_ascii() {
            if let Some(value) = to_align(first as u8) {
                align = value;
                at = first.len_utf8();
            }
        }
    }
    let sign = if matches!(bytes.get(at), Some(b'+') | Some(b'-')) {
        let value = bytes[at];
        at += 1;
        Some(value)
    } else {
        None
    };
    let alternate = bytes.get(at) == Some(&b'#');
    if alternate {
        at += 1;
    }
    let zero = bytes.get(at) == Some(&b'0');
    if zero {
        at += 1;
    }
    let (width, next) = parse_count(bytes, at, false, span)?;
    at = next;
    let precision = if bytes.get(at) == Some(&b'.') {
        at += 1;
        let (value, next) = parse_count(bytes, at, true, span)?;
        at = next;
        if matches!(value, CountSpec::None) {
            return Err(syn::Error::new(span, "precision requires a count"));
        }
        value
    } else {
        CountSpec::None
    };
    let ty = std::str::from_utf8(&bytes[at..]).expect("ASCII").to_owned();
    if !matches!(
        ty.as_str(),
        "" | "?" | "x?" | "X?" | "x" | "X" | "o" | "b" | "e" | "E" | "p"
    ) {
        return Err(syn::Error::new(span, "unsupported or invalid format type"));
    }
    Ok(FormatSpec {
        present: true,
        fill,
        align,
        sign,
        alternate,
        zero,
        width,
        precision,
        ty,
    })
}

fn parse_count(
    bytes: &[u8],
    mut at: usize,
    precision: bool,
    span: Span,
) -> Result<(CountSpec, usize)> {
    if precision && bytes.get(at) == Some(&b'*') {
        return Ok((CountSpec::Star, at + 1));
    }
    let start = at;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    if at > start {
        let text = std::str::from_utf8(&bytes[start..at]).expect("digits");
        let value = text
            .parse::<usize>()
            .map_err(|_| syn::Error::new(span, "format count is too large"))?;
        if bytes.get(at) == Some(&b'$') {
            return Ok((CountSpec::Argument(ArgRef::Index(value)), at + 1));
        }
        return Ok((CountSpec::Literal(value), at));
    }
    if let Some(relative_dollar) = bytes[start..].iter().position(|&byte| byte == b'$') {
        let end = start + relative_dollar;
        if let Ok(name) = std::str::from_utf8(&bytes[start..end]) {
            if !name.is_empty() && syn::parse_str::<Ident>(name).is_ok() {
                return Ok((CountSpec::Argument(ArgRef::Name(name.to_owned())), end + 1));
            }
        }
    }
    Ok((CountSpec::None, start))
}

struct EvaluatedArg {
    expression: Expr,
    name: Option<String>,
    used: bool,
}

struct Resolver {
    args: Vec<EvaluatedArg>,
    positional: Vec<usize>,
    named: HashMap<String, usize>,
    implicit: usize,
    span: Span,
}

impl Resolver {
    fn new(invocation: Vec<InvocationArg>, span: Span) -> Result<Self> {
        let mut args = Vec::new();
        let mut positional = Vec::new();
        let mut named = HashMap::new();
        let mut saw_named = false;
        for argument in invocation {
            let index = args.len();
            if let Some(name) = &argument.name {
                saw_named = true;
                let key = name.to_string();
                if named.insert(key.clone(), index).is_some() {
                    return Err(syn::Error::new_spanned(
                        name,
                        "duplicate named format argument",
                    ));
                }
                args.push(EvaluatedArg {
                    expression: argument.expression,
                    name: Some(key),
                    used: false,
                });
            } else {
                if saw_named {
                    return Err(syn::Error::new_spanned(
                        &argument.expression,
                        "positional arguments cannot follow named arguments",
                    ));
                }
                positional.push(index);
                args.push(EvaluatedArg {
                    expression: argument.expression,
                    name: None,
                    used: false,
                });
            }
        }
        Ok(Self {
            args,
            positional,
            named,
            implicit: 0,
            span,
        })
    }

    fn resolve(&mut self, reference: ArgRef) -> Result<usize> {
        let index = match reference {
            ArgRef::Index(position) => *self.positional.get(position).ok_or_else(|| {
                syn::Error::new(self.span, "format argument index is out of range")
            })?,
            ArgRef::Name(name) => {
                if let Some(&index) = self.named.get(&name) {
                    index
                } else {
                    let mut ident = syn::parse_str::<Ident>(&name)
                        .map_err(|_| syn::Error::new(self.span, "invalid captured identifier"))?;
                    ident.set_span(self.span);
                    let expression: Expr = syn::parse_quote_spanned!(self.span=> #ident);
                    let index = self.args.len();
                    self.args.push(EvaluatedArg {
                        expression,
                        name: Some(name.clone()),
                        used: false,
                    });
                    self.named.insert(name, index);
                    index
                }
            }
        };
        self.args[index].used = true;
        Ok(index)
    }

    fn next_implicit(&mut self) -> Result<usize> {
        let position = self.implicit;
        self.implicit += 1;
        self.resolve(ArgRef::Index(position))
    }
}

struct ResolvedPlaceholder {
    value: usize,
    width: ResolvedCount,
    precision: ResolvedCount,
    spec: FormatSpec,
}

#[derive(Clone)]
enum ResolvedCount {
    None,
    Literal(usize),
    Argument(usize),
}

fn resolve_count(count: &CountSpec, resolver: &mut Resolver) -> Result<ResolvedCount> {
    match count {
        CountSpec::None => Ok(ResolvedCount::None),
        CountSpec::Literal(value) => Ok(ResolvedCount::Literal(*value)),
        CountSpec::Argument(reference) => resolver
            .resolve(reference.clone())
            .map(ResolvedCount::Argument),
        CountSpec::Star => resolver.next_implicit().map(ResolvedCount::Argument),
    }
}

enum ResolvedSegment {
    Literal(Vec<u8>),
    Placeholder(ResolvedPlaceholder),
}

fn expand_inner(input: CommonInput, kind: Kind) -> Result<TokenStream2> {
    let parsed = parse_format(&input.format)?;
    let (_, span, _) = format_bytes(&input.format)?;
    let mut resolver = Resolver::new(input.args, span)?;
    let mut segments = Vec::new();
    for segment in parsed {
        match segment {
            Segment::Literal(bytes) => segments.push(ResolvedSegment::Literal(bytes)),
            Segment::Placeholder(placeholder) => {
                // `.*` consumes its positional argument before an implicit value.
                let precision = resolve_count(&placeholder.spec.precision, &mut resolver)?;
                let value = match placeholder.argument {
                    Some(reference) => resolver.resolve(reference)?,
                    None => resolver.next_implicit()?,
                };
                let width = resolve_count(&placeholder.spec.width, &mut resolver)?;
                segments.push(ResolvedSegment::Placeholder(ResolvedPlaceholder {
                    value,
                    width,
                    precision,
                    spec: placeholder.spec,
                }));
            }
        }
    }
    for argument in &resolver.args {
        if !argument.used {
            let message = match &argument.name {
                Some(name) => format!("named argument `{name}` is never used"),
                None => "positional argument is never used".to_owned(),
            };
            return Err(syn::Error::new_spanned(&argument.expression, message));
        }
    }

    let path = &input.crate_path;
    let bindings: Vec<Ident> = (0..resolver.args.len())
        .map(|index| format_ident!("__cfmt_arg_{index}", span = Span::mixed_site()))
        .collect();
    let expressions: Vec<&Expr> = resolver
        .args
        .iter()
        .map(|argument| &argument.expression)
        .collect();
    let render_body = render_body(&segments, &bindings, path);
    let writer = Ident::new("__cfmt_writer", Span::mixed_site());
    let render = quote! {
        |#writer: &mut dyn ::std::io::Write| -> ::std::io::Result<()> {
            #render_body
            ::std::result::Result::Ok(())
        }
    };
    // Fill the arm separately; keeping the borrowed tuple is what extends
    // temporary lifetimes and guarantees one left-to-right evaluation.
    let with_args = |body: TokenStream2| {
        quote! {
            match (#(&(#expressions),)*) {
                (#(#bindings,)*) => { #body }
            }
        }
    };
    let result = match kind {
        Kind::Printf => with_args(quote! { #path::__private::emit(#path::CStdout, #render) }),
        Kind::Eprintf => with_args(quote! { #path::__private::emit(#path::CStderr, #render) }),
        Kind::Emit => {
            let destination = input.destination.expect("emit has destination");
            let sink = Ident::new("__cfmt_sink", Span::mixed_site());
            let inner = with_args(quote! { #path::__private::emit(#sink, #render) });
            quote! { match (#destination) { #sink => { #inner } } }
        }
        Kind::Write => {
            let destination = input.destination.expect("write has destination");
            let rendered = with_args(render.clone());
            quote! {{
                use #path::__private::WriteExt as _;
                (#destination).__xj_cfmt_write(#rendered)
            }}
        }
        Kind::Snprintf => {
            let destination = input.destination.expect("snprintf has destination");
            let destination_view = Ident::new("__cfmt_destination", Span::mixed_site());
            let inner = with_args(quote! {
                #path::__private::snprintf_dest(#destination_view, #render)
            });
            quote! {{
                use #path::CStrBuffer as _;
                match (#destination).cstr_dest() {
                    #destination_view => { #inner }
                }
            }}
        }
    };
    Ok(result)
}

fn render_body(segments: &[ResolvedSegment], bindings: &[Ident], path: &Path) -> TokenStream2 {
    let writer = Ident::new("__cfmt_writer", Span::mixed_site());
    let operations = segments.iter().map(|segment| match segment {
        ResolvedSegment::Literal(bytes) => {
            let bytes = syn::LitByteStr::new(bytes, Span::call_site());
            quote! { ::std::io::Write::write_all(#writer, #bytes)?; }
        }
        ResolvedSegment::Placeholder(placeholder) if placeholder.spec.ty.is_empty() => {
            display_operation(placeholder, bindings, path, &writer)
        }
        ResolvedSegment::Placeholder(placeholder) => {
            std_operation(placeholder, bindings, path, &writer)
        }
    });
    quote! { #(#operations)* }
}

fn count_expression(count: &ResolvedCount, bindings: &[Ident], path: &Path) -> TokenStream2 {
    match count {
        ResolvedCount::None => quote! { ::std::option::Option::None },
        ResolvedCount::Literal(value) => quote! { ::std::option::Option::Some(#value) },
        ResolvedCount::Argument(index) => {
            let binding = &bindings[*index];
            quote! { ::std::option::Option::Some(#path::__private::count(#binding)) }
        }
    }
}

fn alignment_expression(alignment: Alignment, path: &Path) -> TokenStream2 {
    let variant = match alignment {
        Alignment::Unspecified => Ident::new("Unspecified", Span::call_site()),
        Alignment::Left => Ident::new("Left", Span::call_site()),
        Alignment::Right => Ident::new("Right", Span::call_site()),
        Alignment::Center => Ident::new("Center", Span::call_site()),
    };
    quote! { #path::__private::Align::#variant }
}

fn display_operation(
    placeholder: &ResolvedPlaceholder,
    bindings: &[Ident],
    path: &Path,
    writer: &Ident,
) -> TokenStream2 {
    let value = &bindings[placeholder.value];
    let fill = placeholder.spec.fill;
    let align = alignment_expression(placeholder.spec.align, path);
    let width = count_expression(&placeholder.width, bindings, path);
    let precision = count_expression(&placeholder.precision, bindings, path);
    let (format_string, count_args) = canonical_format(placeholder, bindings, path);
    quote! {
        {
            use #path::__private::{ViaBytes as _, ViaStd as _};
            (&&#path::__private::Arg(#value)).fmt_display(
                #writer,
                &#path::__private::Spec {
                    fill: #fill,
                    align: #align,
                    width: #width,
                    precision: #precision,
                },
                &mut |__cfmt_std_writer: &mut dyn ::std::io::Write,
                      __cfmt_value: &dyn ::std::fmt::Display|
                      -> ::std::io::Result<()> {
                    #path::__private::write_fmt(
                        __cfmt_std_writer,
                        ::std::format_args!(#format_string, __cfmt_value #(, #count_args)*)
                    )
                },
            )?;
        }
    }
}

fn std_operation(
    placeholder: &ResolvedPlaceholder,
    bindings: &[Ident],
    path: &Path,
    writer: &Ident,
) -> TokenStream2 {
    let value = &bindings[placeholder.value];
    let (format_string, count_args) = canonical_format(placeholder, bindings, path);
    quote! {
        #path::__private::write_fmt(
            #writer,
            ::std::format_args!(
                #format_string,
                #path::__private::FmtRef(#value) #(, #count_args)*
            ),
        )?;
    }
}

fn canonical_format(
    placeholder: &ResolvedPlaceholder,
    bindings: &[Ident],
    path: &Path,
) -> (String, Vec<TokenStream2>) {
    let mut result = String::from("{0");
    let mut count_args = Vec::new();
    if placeholder.spec.present {
        result.push(':');
        match placeholder.spec.align {
            Alignment::Unspecified => {}
            Alignment::Left | Alignment::Right | Alignment::Center => {
                if placeholder.spec.fill != b' ' {
                    result.push(placeholder.spec.fill as char);
                }
                result.push(match placeholder.spec.align {
                    Alignment::Left => '<',
                    Alignment::Right => '>',
                    Alignment::Center => '^',
                    Alignment::Unspecified => unreachable!(),
                });
            }
        }
        if let Some(sign) = placeholder.spec.sign {
            result.push(sign as char);
        }
        if placeholder.spec.alternate {
            result.push('#');
        }
        if placeholder.spec.zero {
            result.push('0');
        }
        push_count(
            &mut result,
            &mut count_args,
            &placeholder.width,
            bindings,
            path,
            false,
        );
        push_count(
            &mut result,
            &mut count_args,
            &placeholder.precision,
            bindings,
            path,
            true,
        );
        result.push_str(&placeholder.spec.ty);
    }
    result.push('}');
    (result, count_args)
}

fn push_count(
    output: &mut String,
    args: &mut Vec<TokenStream2>,
    count: &ResolvedCount,
    bindings: &[Ident],
    path: &Path,
    precision: bool,
) {
    if !matches!(count, ResolvedCount::None) && precision {
        output.push('.');
    }
    match count {
        ResolvedCount::None => {}
        ResolvedCount::Literal(value) => output.push_str(&value.to_string()),
        ResolvedCount::Argument(index) => {
            let position = args.len() + 1;
            output.push_str(&position.to_string());
            output.push('$');
            let binding = &bindings[*index];
            args.push(quote! { #path::__private::count(#binding) });
        }
    }
}

/// Validate the safety effects of a literal C format. Runtime formats are
/// checked by render_safe; argument types are checked by SafeAdapt.
#[proc_macro]
pub fn validate_safe_format(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let expr = match syn::parse::<syn::Expr>(tokens) {
        Ok(expr) => expr,
        Err(e) => return e.into_compile_error().into(),
    };
    let mut literal = &expr;
    loop {
        match literal {
            syn::Expr::Group(g) => literal = &g.expr,
            syn::Expr::Paren(p) => literal = &p.expr,
            _ => break,
        }
    }
    let syn::Expr::Lit(lit) = literal else {
        return proc_macro::TokenStream::new();
    };
    let bytes = match &lit.lit {
        syn::Lit::ByteStr(b) => b.value(),
        syn::Lit::Str(s) => s.value().into_bytes(),
        syn::Lit::CStr(s) => s.value().to_bytes().to_vec(),
        _ => return proc_macro::TokenStream::new(),
    };
    match xj_cfmt_core::Plan::new(&bytes) {
        Ok(plan) if !plan.count_stores => proc_macro::TokenStream::new(),
        Ok(_) => syn::Error::new_spanned(expr, "%n requires the unsafe C formatting form")
            .into_compile_error()
            .into(),
        Err(e) => syn::Error::new_spanned(expr, e.to_string())
            .into_compile_error()
            .into(),
    }
}
