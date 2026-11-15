/**
 * Style gate: authored functions stay at or below 30 physical lines and exported
 * function APIs have a TSDoc block immediately above them.
 */
import fs from "node:fs";
import path from "node:path";
import ts from "typescript";

const root = path.resolve(import.meta.dirname, "..");
const config = ts.readConfigFile(path.join(root, "tsconfig.json"), ts.sys.readFile);
const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, root);
const files = parsed.fileNames.filter((file) => /\.(ts|tsx)$/.test(file) && !file.endsWith(".d.ts") && !file.includes(".test."));
const violations = [];

for (const file of files) {
  const sourceText = fs.readFileSync(file, "utf8");
  const source = ts.createSourceFile(file, sourceText, ts.ScriptTarget.Latest, true, file.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const visit = (node) => {
    if (isFunctionWithBody(node)) {
      const start = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
      const end = source.getLineAndCharacterOfPosition(node.getEnd() - 1).line + 1;
      const length = end - start + 1;
      if (length > 30) {
        violations.push(`${path.relative(root, file)}:${start} ${functionName(node)} is ${length} lines (max 30)`);
      }
      if (isExportedApi(node) && !hasLeadingTsDoc(node, sourceText)) {
        violations.push(`${path.relative(root, file)}:${start} exported function lacks a leading TSDoc block`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
}

if (violations.length > 0) {
  console.error("Frontend style gate failed:\n" + violations.map((item) => `- ${item}`).join("\n"));
  process.exitCode = 1;
} else {
  console.log(`Style gate passed for ${files.length} TS/TSX files.`);
}

function functionName(node) {
  if (node.name?.getText) return node.name.getText();
  const parent = node.parent;
  if (parent && ts.isVariableDeclaration(parent) && parent.name?.getText) return parent.name.getText();
  return "<anonymous>";
}

function isFunctionWithBody(node) {
  if (!node.body) return false;
  return (
    ts.isFunctionDeclaration(node) ||
    ts.isFunctionExpression(node) ||
    ts.isArrowFunction(node) ||
    ts.isMethodDeclaration(node) ||
    ts.isConstructorDeclaration(node) ||
    ts.isGetAccessorDeclaration(node) ||
    ts.isSetAccessorDeclaration(node)
  );
}

function isExportedApi(node) {
  if (ts.isFunctionDeclaration(node) && node.modifiers?.some((modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword)) return true;
  const parent = node.parent;
  if (!parent) return false;
  if (ts.isVariableDeclaration(parent) && parent.parent?.parent && ts.isVariableStatement(parent.parent.parent)) {
    return Boolean(parent.parent.parent.modifiers?.some((modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword));
  }
  return false;
}

function hasLeadingTsDoc(node, sourceText) {
  const leading = sourceText.slice(node.getFullStart(), node.getStart()).trimEnd();
  return /\/\*\*[\s\S]*?\*\/\s*$/.test(leading);
}
