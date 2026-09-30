import { test, expect } from "@playwright/test";

for (const client of ["web", "desktop"]) {
  test(`${client}: text assistance stays off for initial and dynamic inputs`, async ({ page }) => {
    await page.goto(client === "web" ? "/" : "http://127.0.0.1:1431");
    if (client === "desktop") await page.getByRole("radio", { name: "远程协作", exact: true }).click();
    await expect(page.getByLabel("用户名", { exact: true })).toBeVisible();
    const username = page.getByLabel("用户名", { exact: true });
    await expect(username).toHaveAttribute("autocapitalize", "off");
    await expect(username).toHaveAttribute("autocorrect", "off");
    await expect(username).toHaveAttribute("spellcheck", "false");

    // A later dialog can insert a whole subtree or a single editable element.
    await page.evaluate(() => {
      const dialog = document.createElement("section");
      dialog.id = "input-defaults-fixture";
      dialog.innerHTML = '<input aria-label="dynamic name"><textarea aria-label="dynamic notes"></textarea><div contenteditable="true" aria-label="dynamic editor"></div>';
      document.body.append(dialog);
      const input = document.createElement("input");
      input.setAttribute("aria-label", "later input");
      dialog.append(input);
    });
    for (const label of ["dynamic name", "dynamic notes", "dynamic editor", "later input"]) {
      const input = page.getByLabel(label, { exact: true });
      await expect(input).toHaveAttribute("autocapitalize", "off");
      await expect(input).toHaveAttribute("autocorrect", "off");
      await expect(input).toHaveAttribute("spellcheck", "false");
    }
    await page.getByLabel("dynamic notes").fill("children teh lowerCase 资源路径");
    await page.getByLabel("dynamic name").focus();
    await expect(page.getByLabel("dynamic notes")).toHaveValue("children teh lowerCase 资源路径");
  });
}
