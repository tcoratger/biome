use crate::lexer::{JsLexContext, JsLexer, JsReLexContext, TextRange};
use crate::prelude::*;
use biome_js_syntax::JsSyntaxKind;
use biome_js_syntax::JsSyntaxKind::EOF;
use biome_parser::lexer::{BufferedLexer, LexContext, LexerCheckpoint};
use biome_parser::token_source::Trivia;
use biome_rowan::{TextSize, TriviaPieceKind};
use std::collections::VecDeque;

/// Token source for the parser that skips over any non-trivia token.
pub struct JsTokenSource<'l> {
    lexer: BufferedLexer<'l, JsLexer<'l>>,

    /// List of the skipped trivia. Needed to construct the CST and compute the non-trivia token offsets.
    pub(super) trivia_list: Vec<Trivia>,

    /// Cache for the non-trivia token lookahead. For example for the source `let a = 10;` if the
    /// [TokenSource]'s currently positioned at the start of the file (`let`). The `nth(2)` non-trivia token,
    /// as returned by the [TokenSource], is the `=` token but retrieving it requires skipping over the
    /// two whitespace trivia tokens (first between `let` and `a`, second between `a` and `=`).
    /// The [TokenSource] state then is:
    ///
    /// * `non_trivia_lookahead`: [IDENT: 'a', EQ]
    /// * `lookahead_offset`: 4 (the `=` is the 4th token after the `let` keyword)
    non_trivia_lookahead: VecDeque<Lookahead>,

    /// Offset of the last cached lookahead token from the current [BufferedLexer] token.
    lookahead_offset: usize,
}

#[derive(Debug, Copy, Clone)]
struct Lookahead {
    kind: JsSyntaxKind,
    after_newline: bool,
}

impl<'l> JsTokenSource<'l> {
    /// Creates a new token source.
    pub(crate) fn new(lexer: BufferedLexer<'l, JsLexer<'l>>) -> JsTokenSource<'l> {
        JsTokenSource {
            lexer,
            trivia_list: vec![],
            lookahead_offset: 0,
            non_trivia_lookahead: VecDeque::new(),
        }
    }

    /// Creates a new token source for the given string
    pub fn from_str(source: &'l str) -> JsTokenSource<'l> {
        let lexer = JsLexer::from_str(source);
        let buffered = BufferedLexer::new(lexer);
        let mut source = JsTokenSource::new(buffered);

        source.next_non_trivia_token(JsLexContext::default(), true);
        source
    }

    #[inline]
    fn next_non_trivia_token(&mut self, context: JsLexContext, first_token: bool) {
        let mut processed_tokens = 0;
        let mut trailing = !first_token;

        // Drop the last cached lookahead, we're now moving past it
        self.non_trivia_lookahead.pop_front();

        loop {
            let kind = self.lexer.next_token(context);
            processed_tokens += 1;

            let trivia_kind = TriviaPieceKind::try_from(kind);

            match trivia_kind {
                Err(_) => break,
                Ok(trivia_kind) => {
                    // Trivia after and including the newline is considered the leading trivia of the next token
                    if trivia_kind.is_newline() {
                        trailing = false;
                    }

                    self.trivia_list
                        .push(Trivia::new(trivia_kind, self.current_range(), trailing));
                }
            }
        }

        if self.lookahead_offset != 0 {
            debug_assert!(self.lookahead_offset >= processed_tokens);
            self.lookahead_offset -= processed_tokens;
        }
    }

    #[inline(always)]
    pub fn has_unicode_escape(&self) -> bool {
        self.lexer.has_unicode_escape()
    }

    /// Returns `true` if the next token has any preceding trivia (either trailing trivia of the current
    /// token or leading trivia of the next token)
    pub fn has_next_preceding_trivia(&mut self) -> bool {
        let next_token_trivia = self
            .lexer
            .lookahead()
            .next()
            .and_then(|lookahead| TriviaPieceKind::try_from(lookahead.kind()).ok());
        next_token_trivia.is_some()
    }

    #[inline(always)]
    fn lookahead(&mut self, n: usize) -> Option<Lookahead> {
        assert_ne!(n, 0);

        // Return the cached token if any
        if let Some(lookahead) = self.non_trivia_lookahead.get(n - 1) {
            return Some(*lookahead);
        }

        // Jump right to where we've left of last time rather than going through all tokens again.
        let iter = self.lexer.lookahead().skip(self.lookahead_offset);
        let mut remaining = n - self.non_trivia_lookahead.len();

        for item in iter {
            self.lookahead_offset += 1;

            if !item.kind().is_trivia() {
                remaining -= 1;

                let lookahead = Lookahead {
                    after_newline: item.has_preceding_line_break(),
                    kind: item.kind(),
                };

                self.non_trivia_lookahead.push_back(lookahead);

                if remaining == 0 {
                    return Some(lookahead);
                }
            }
        }

        None
    }

    pub fn re_lex(&mut self, mode: JsReLexContext) -> JsSyntaxKind {
        let current_kind = self.current();

        let new_kind = self.lexer.re_lex(mode);

        // Only need to clear the lookahead cache when the token did change
        if current_kind != new_kind {
            self.non_trivia_lookahead.clear();
            self.lookahead_offset = 0;
        }

        new_kind
    }

    /// Creates a checkpoint to which it can later return using [Self::rewind].
    pub fn checkpoint(&self) -> TokenSourceCheckpoint {
        TokenSourceCheckpoint {
            trivia_len: self.trivia_list.len() as u32,
            lexer: self.lexer.checkpoint(),
        }
    }

    /// Restores the token source to a previous state
    pub fn rewind(&mut self, checkpoint: TokenSourceCheckpoint) {
        assert!(self.trivia_list.len() >= checkpoint.trivia_len as usize);
        self.trivia_list.truncate(checkpoint.trivia_len as usize);
        self.lexer.rewind(checkpoint.lexer);
        self.non_trivia_lookahead.clear();
        self.lookahead_offset = 0;
    }
}

impl<'source> TokenSource for JsTokenSource<'source> {
    type Kind = JsSyntaxKind;

    /// Returns the kind of the current non-trivia token
    #[inline(always)]
    fn current(&self) -> JsSyntaxKind {
        self.lexer.current()
    }

    /// Returns the range of the current non-trivia token
    #[inline(always)]
    fn current_range(&self) -> TextRange {
        self.lexer.current_range()
    }

    #[inline(always)]
    fn text(&self) -> &'source str {
        self.lexer.source()
    }

    #[inline(always)]
    fn position(&self) -> TextSize {
        self.current_range().start()
    }

    /// Returns true if the current token is preceded by a line break
    #[inline(always)]
    fn has_preceding_line_break(&self) -> bool {
        self.lexer.has_preceding_line_break()
    }

    #[inline(always)]
    fn bump(&mut self) {
        self.bump_with_context(JsLexContext::Regular)
    }

    fn skip_as_trivia(&mut self) {
        self.skip_as_trivia_with_context(JsLexContext::Regular)
    }

    fn finish(self) -> (Vec<Trivia>, Vec<ParseDiagnostic>) {
        (self.trivia_list, self.lexer.finish())
    }
}

impl<'source> BumpWithContext for JsTokenSource<'source> {
    type Context = JsLexContext;

    #[inline(always)]
    fn bump_with_context(&mut self, context: Self::Context) {
        if self.current() != EOF {
            if !context.is_regular() {
                self.lookahead_offset = 0;
                self.non_trivia_lookahead.clear();
            }

            self.next_non_trivia_token(context, false);
        }
    }

    /// Skips the current token as skipped token trivia
    fn skip_as_trivia_with_context(&mut self, context: JsLexContext) {
        if self.current() != EOF {
            if !context.is_regular() {
                self.lookahead_offset = 0;
                self.non_trivia_lookahead.clear();
            }

            self.trivia_list.push(Trivia::new(
                TriviaPieceKind::Skipped,
                self.current_range(),
                false,
            ));

            self.next_non_trivia_token(context, true)
        }
    }
}

impl<'source> NthToken for JsTokenSource<'source> {
    /// Gets the kind of the nth non-trivia token
    #[inline(always)]
    fn nth(&mut self, n: usize) -> JsSyntaxKind {
        if n == 0 {
            self.current()
        } else {
            self.lookahead(n).map_or(EOF, |lookahead| lookahead.kind)
        }
    }

    /// Returns true if the nth non-trivia token is preceded by a line break
    #[inline(always)]
    fn has_nth_preceding_line_break(&mut self, n: usize) -> bool {
        if n == 0 {
            self.has_preceding_line_break()
        } else {
            self.lookahead(n)
                .map_or(false, |lookahead| lookahead.after_newline)
        }
    }
}

#[derive(Debug)]
pub struct TokenSourceCheckpoint {
    lexer: LexerCheckpoint<JsSyntaxKind>,
    /// A `u32` should be enough because `TextSize` is also limited to `u32`.
    /// The worst case is a document where every character is its own token. This would
    /// result in `u32::MAX` tokens
    trivia_len: u32,
}

impl TokenSourceCheckpoint {
    /// byte offset in the source text
    pub(crate) fn current_start(&self) -> TextSize {
        self.lexer.current_start()
    }

    pub(crate) fn trivia_position(&self) -> usize {
        self.trivia_len as usize
    }
}
