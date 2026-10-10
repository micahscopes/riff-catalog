//! The declarations a catalog knows: one per recipe.
//!
//! An application declares its recipes when it opens a corpus; the
//! recipe-free tool declares the files it is given. The corpus stores the
//! declaration of every recipe it holds units of, so a later run cannot read
//! those units under a different one.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use riff_catalog_core::{Dimension, Graph, RecipeDecl, check_recipe};

#[derive(Clone, Debug, Default)]
pub struct Declarations {
    by_recipe: BTreeMap<String, RecipeDecl>,
}

impl Declarations {
    /// Add a declaration. Adding the same declaration twice is fine; two
    /// different declarations for one recipe are refused, in either order.
    pub fn declare(&mut self, decl: RecipeDecl) -> Result<()> {
        if let Err(problems) = decl.validate() {
            bail!("declaration of {} is not well formed: {}", decl.recipe.as_str(), problems.join("; "));
        }
        match self.by_recipe.get(decl.recipe.as_str()) {
            Some(existing) if *existing != decl => {
                bail!("two different declarations for recipe {}", decl.recipe.as_str())
            }
            Some(_) => {}
            None => {
                self.by_recipe.insert(decl.recipe.as_str().to_string(), decl);
            }
        }
        Ok(())
    }

    /// The declaration of a recipe, when there is one.
    pub fn get(&self, recipe: &str) -> Option<&RecipeDecl> {
        self.by_recipe.get(recipe)
    }

    /// Every declared recipe, in name order.
    pub fn recipes(&self) -> impl Iterator<Item = &str> {
        self.by_recipe.keys().map(String::as_str)
    }

    /// The dimensions a recipe's units are hashed in: the ones its declaration
    /// fills. An unfilled dimension has no meaning for the recipe and gets no
    /// digest.
    pub fn dimensions(&self, recipe: &str) -> Result<Vec<Dimension>> {
        Ok(self.require(recipe)?.fills().into_iter().collect())
    }

    /// The declaration of a recipe, or an error naming the recipe.
    pub fn require(&self, recipe: &str) -> Result<&RecipeDecl> {
        self.get(recipe).ok_or_else(|| anyhow::anyhow!("no declaration for recipe {recipe}"))
    }

    /// Refuse a graph of an undeclared recipe, or one that breaks its
    /// recipe's declaration.
    pub fn check(&self, recipe: &str, name: &str, graph: &Graph) -> Result<()> {
        if let Err(violations) = check_recipe(self.require(recipe)?, graph) {
            let shown: Vec<String> = violations
                .iter()
                .take(5)
                .map(|v| format!("  {}{}", v.at.as_deref().map(|at| format!("{at}: ")).unwrap_or_default(), v.problem))
                .collect();
            bail!(
                "{name} breaks the {recipe} declaration ({} problems):\n{}",
                violations.len(),
                shown.join("\n")
            );
        }
        Ok(())
    }
}
