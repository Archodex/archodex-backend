#!/bin/bash -xe

# docker run --name archodex-dynamodb-local -p 8001:8000 -d amazon/dynamodb-local:latest -jar DynamoDBLocal.jar -sharedDb

export AWS_REGION=us-west-2
export AWS_ACCESS_KEY_ID=invalid
export AWS_SECRET_ACCESS_KEY=invalid
export AWS_ENDPOINT_URL=http://localhost:8001

aws dynamodb create-table \
    --table-name archodex-accounts \
    --attribute-definitions AttributeName=pk,AttributeType=B AttributeName=sk,AttributeType=B \
    --key-schema AttributeName=pk,KeyType=HASH AttributeName=sk,KeyType=RANGE \
    --billing-mode PAY_PER_REQUEST