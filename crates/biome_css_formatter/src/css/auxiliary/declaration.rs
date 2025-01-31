use crate::prelude::*;
use biome_css_syntax::{CssDeclaration, CssDeclarationFields};
use biome_formatter::write;

#[derive(Debug, Clone, Default)]
pub(crate) struct FormatCssDeclaration;
impl FormatNodeRule<CssDeclaration> for FormatCssDeclaration {
    fn fmt_fields(&self, node: &CssDeclaration, f: &mut CssFormatter) -> FormatResult<()> {
        let CssDeclarationFields {
            name,
            colon_token,
            value,
            important,
        } = node.as_fields();

        write!(
            f,
            [name.format(), colon_token.format(), space(), value.format()]
        )?;

        if important.is_some() {
            write!(f, [space(), important.format()])?;
        }

        Ok(())
    }
}
