#!/usr/bin/env python3
"""Check files and fragment targets in a generated mdBook without network access."""

import argparse
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit


class Page(HTMLParser):
    def __init__(self, path):
        super().__init__()
        self.path = path
        self.ids = set()
        self.links = []
        self.feed(path.read_text(encoding="utf-8"))

    def handle_starttag(self, tag, attributes):
        attributes = dict(attributes)
        if attributes.get("id"):
            self.ids.add(attributes["id"])
        if tag == "a" and attributes.get("name"):
            self.ids.add(attributes["name"])
        if tag in {"a", "link"} and attributes.get("href"):
            self.links.append((attributes["href"], self.getpos()[0]))
        if tag in {"img", "script"} and attributes.get("src"):
            self.links.append((attributes["src"], self.getpos()[0]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("book", type=Path)
    parser.add_argument("--site-url", default="/lictor/")
    arguments = parser.parse_args()
    root = arguments.book.resolve()
    pages = {path.resolve(): Page(path) for path in root.rglob("*.html")}
    if not pages:
        parser.error(f"no HTML pages found in {root}; run mdbook build first")
    errors = []
    prefix = "/" + arguments.site_url.strip("/") + "/"
    for path, page in pages.items():
        for href, line in page.links:
            url = urlsplit(href)
            if url.scheme or url.netloc:
                continue
            local = unquote(url.path)
            if local.startswith(prefix):
                target = root / local[len(prefix):]
            elif local.startswith("/"):
                # A link outside this project's publishing prefix is site-owned.
                continue
            elif local:
                target = path.parent / local
            else:
                target = path
            target = target.resolve()
            if target.is_dir():
                target = target / "index.html"
            if not target.is_file():
                errors.append(f"{path.relative_to(root)}:{line}: missing file: {href}")
            elif url.fragment and target.suffix == ".html":
                target_page = pages.get(target)
                if target_page is None:
                    target_page = Page(target)
                if unquote(url.fragment) not in target_page.ids:
                    errors.append(f"{path.relative_to(root)}:{line}: missing anchor: {href}")
    if errors:
        for error in errors:
            print(error)
        return 1
    print(f"Checked local links and anchors in {len(pages)} generated pages.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
