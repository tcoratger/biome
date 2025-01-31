use crate::prelude::*;
use biome_css_syntax::CssLayerNameList;
#[derive(Debug, Clone, Default)]
pub(crate) struct FormatCssLayerNameList;
impl FormatRule<CssLayerNameList> for FormatCssLayerNameList {
    type Context = CssFormatContext;
    fn fmt(&self, node: &CssLayerNameList, f: &mut CssFormatter) -> FormatResult<()> {
        format_verbatim_node(node.syntax()).fmt(f)
    }
}
