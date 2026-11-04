# Task intents

A TaskIntent is a durable, private request to plan or perform work. Its task
tree, conversation, decisions, approvals, worker activity, and QA evidence are
stored in the local application data root, outside code repositories and task
clones.

Task records do not synchronize between installations. Workers may use
isolated task clones, but task clones contain only the code needed to perform
their work. Human approval and independent QA remain required by the applicable
workflow gates.
