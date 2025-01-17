# Spider CLI

![crate version](https://img.shields.io/crates/v/spider.svg)

A fast command line spider or crawler.

## Dependencies

On Linux

- OpenSSL 1.0.1, 1.0.2, 1.1.0, or 1.1.1

## Usage

The CLI is a binary so do not add it to your `Cargo.toml` file.

```sh
cargo install spider_cli
```

## Cli

The following can also be ran via command line to run the crawler.
If you need loging pass in the `-v` flag.

```sh
spider -v --domain https://choosealicense.com crawl
```

Crawl and output all links visited to a file.

```sh
spider --domain https://choosealicense.com crawl -o > spider_choosealicense.json
```

Download all html to local destination. Use the option `-t` to pass in the target destination folder.

```sh
spider --domain https://choosealicense.com download -t _temp_spider_downloads
```

Set a crawl budget and only crawl one domain.

```sh
spider --domain https://choosealicense.com --budget "*,1" crawl -o
```

Set a crawl budget and only allow 10 pages matching the /blog/ path and limit all pages to 100.

```sh
spider --domain https://choosealicense.com --budget "*,100,/blog/,10" crawl -o
```

```sh
The fastest web crawler CLI written in Rust.

Usage: spider [OPTIONS] --domain <DOMAIN> [COMMAND]

Commands:
  crawl     Crawl the website extracting links
  scrape    Scrape the website extracting html and links
  download  Download html markup to destination
  help      Print this message or the help of the given subcommand(s)

Options:
  -d, --domain <DOMAIN>                Domain to crawl
  -r, --respect-robots-txt             Respect robots.txt file
  -s, --subdomains                     Allow sub-domain crawling
  -t, --tld                            Allow all tlds for domain
  -v, --verbose                        Print page visited on standard output
  -D, --delay <DELAY>                  Polite crawling delay in milli seconds
  -b, --blacklist-url <BLACKLIST_URL>  Comma seperated string list of pages to not crawl or regex with feature enabled
  -u, --user-agent <USER_AGENT>        User-Agent
  -B, --budget <BUDGET>                Crawl Budget
  -h, --help                           Print help
  -V, --version                        Print version
```

All features are available except the Website struct `on_link_find_callback` configuration option.
