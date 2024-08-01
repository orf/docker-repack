FROM ollama/ollama
COPY --from=models /blobs/ /models/
