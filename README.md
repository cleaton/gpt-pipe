# gpt-pipe
gpt-pipe is a proof-of-concept tool designed to simplify the creation of bespoke scripts for tasks such as log analytics. Utilizing Unix pipes, it allows users to generate and execute one-off scripts efficiently.

![](docs/gpt-pipe-demo.gif)

For example, you could find the total time spent on compaction:

```
cat scylla.log | grep "Compacted" | cargo run "What is the total time spent on compaction?"
Total time spent on compaction: 8206ms
```

Or identify the number of unique IP addresses:

```
cat nginx-access.log | cargo run "How many unique IPs are there?"
6
```

Before using gpt-pipe, ensure you have an OpenAI API token. This token should be stored in a `.token` file.

## Design

gpt-pipe operates following these steps:

1. Initiate a system prompt, which asks for the creation of a user-defined solution based on a sample of the standard input data.
2. Save the code generated to a file, allowing the user to modify it or rerun the program without invoking GPT again.
3. Execute the JavaScript code within a sandbox provided by Deno Core. This adds a layer of security by preventing potentially harmful code returned by GPT from gaining external access.

With gpt-pipe, you have a simple, efficient, and secure tool for quick log analysis and more at your fingertips.