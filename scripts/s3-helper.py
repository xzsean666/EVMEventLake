#!/usr/bin/env python3
"""
Zero-dependency S3 / MinIO / Cloudflare R2 helper using standard library urllib and SigV4.
Supports PUT (upload file), GET (download file), LIST (list keys), and HEAD.
"""

import sys
import os
import datetime
import hashlib
import hmac
import urllib.request
import urllib.parse
import urllib.error
import xml.etree.ElementTree as ET

def sign(key, msg):
    return hmac.new(key, msg.encode('utf-8'), hashlib.sha256).digest()

def get_signature_key(key, date_stamp, region_name, service_name):
    k_date = sign(('AWS4' + key).encode('utf-8'), date_stamp)
    k_region = sign(k_date, region_name)
    k_service = sign(k_region, service_name)
    k_signing = sign(k_service, 'aws4_request')
    return k_signing

def send_s3_request(method, endpoint, bucket, key, access_key, secret_key, region='us-east-1', data=None, content_type='application/octet-stream'):
    endpoint = endpoint.rstrip('/')
    if not endpoint.startswith('http://') and not endpoint.startswith('https://'):
        endpoint = 'https://' + endpoint

    parsed = urllib.parse.urlparse(endpoint)
    host = parsed.netloc

    # Path style vs virtual hosted style: for custom endpoints (MinIO/IP) path style is common
    path = f"/{bucket}/{key.lstrip('/')}"
    url = f"{endpoint}{path}"

    t = datetime.datetime.now(datetime.timezone.utc)
    amz_date = t.strftime('%Y%m%d%T%Z').replace('UTC', 'Z').replace('GMT', 'Z')
    date_stamp = t.strftime('%Y%m%d')

    payload_hash = hashlib.sha256(data if data else b'').hexdigest()

    canonical_uri = urllib.parse.quote(path)
    canonical_querystring = ''
    canonical_headers = f'host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n'
    signed_headers = 'host;x-amz-content-sha256;x-amz-date'

    canonical_request = f"{method}\n{canonical_uri}\n{canonical_querystring}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"

    algorithm = 'AWS4-HMAC-SHA256'
    credential_scope = f"{date_stamp}/{region}/s3/aws4_request"
    string_to_sign = f"{algorithm}\n{amz_date}\n{credential_scope}\n{hashlib.sha256(canonical_request.encode('utf-8')).hexdigest()}"

    signing_key = get_signature_key(secret_key, date_stamp, region, 's3')
    signature = hmac.new(signing_key, string_to_sign.encode('utf-8'), hashlib.sha256).hexdigest()

    authorization_header = f"{algorithm} Credential={access_key}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"

    headers = {
        'x-amz-date': amz_date,
        'x-amz-content-sha256': payload_hash,
        'Authorization': authorization_header,
        'Content-Type': content_type
    }

    req = urllib.request.Request(url, data=data if method in ('PUT', 'POST') else None, headers=headers, method=method)
    return urllib.request.urlopen(req, timeout=60)

def upload_file(local_path, endpoint, bucket, key, access_key, secret_key, region='us-east-1'):
    with open(local_path, 'rb') as f:
        data = f.read()
    resp = send_s3_request('PUT', endpoint, bucket, key, access_key, secret_key, region, data=data)
    if resp.status in (200, 204):
        print(f"[OK] Uploaded {local_path} -> s3://{bucket}/{key}")
        return True
    else:
        print(f"[ERROR] Failed to upload {local_path}: HTTP {resp.status}", file=sys.stderr)
        return False

def download_file(local_path, endpoint, bucket, key, access_key, secret_key, region='us-east-1'):
    resp = send_s3_request('GET', endpoint, bucket, key, access_key, secret_key, region)
    os.makedirs(os.path.dirname(os.path.abspath(local_path)), exist_ok=True)
    with open(local_path, 'wb') as f:
        f.write(resp.read())
    print(f"[OK] Downloaded s3://{bucket}/{key} -> {local_path}")
    return True

def main():
    if len(sys.argv) < 2:
        print("Usage: s3-helper.py <upload|download> <local_file> <s3_key>")
        sys.exit(1)

    action = sys.argv[1]
    endpoint = os.environ.get('BACKUP_S3_ENDPOINT', 'https://s3.us-east-1.amazonaws.com')
    bucket = os.environ.get('BACKUP_S3_BUCKET', '')
    region = os.environ.get('BACKUP_S3_REGION', 'us-east-1')
    access_key = os.environ.get('BACKUP_S3_ACCESS_KEY', '')
    secret_key = os.environ.get('BACKUP_S3_SECRET_KEY', '')

    if not bucket or not access_key or not secret_key:
        print("Error: BACKUP_S3_BUCKET, BACKUP_S3_ACCESS_KEY, and BACKUP_S3_SECRET_KEY must be set.", file=sys.stderr)
        sys.exit(1)

    if action == 'upload':
        local_path = sys.argv[2]
        s3_key = sys.argv[3]
        if upload_file(local_path, endpoint, bucket, s3_key, access_key, secret_key, region):
            sys.exit(0)
        sys.exit(1)
    elif action == 'download':
        local_path = sys.argv[2]
        s3_key = sys.argv[3]
        if download_file(local_path, endpoint, bucket, s3_key, access_key, secret_key, region):
            sys.exit(0)
        sys.exit(1)
    else:
        print(f"Unknown action: {action}", file=sys.stderr)
        sys.exit(1)

if __name__ == '__main__':
    main()
