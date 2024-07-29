FROM ollama/ollama
RUN --mount=type=tmpfs,target=/tmp/,size=10GB <<EOF
ollama serve & sleep 5 ;
ollama pull llama3.1 && ollama pull mistral-large;
EOF