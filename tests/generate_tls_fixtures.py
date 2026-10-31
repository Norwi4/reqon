"""Regenerate public, test-only TLS fixtures (requires cryptography on the developer machine)."""
from datetime import datetime,timezone
from pathlib import Path
from cryptography import x509
from cryptography.hazmat.primitives import hashes,serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID,ExtendedKeyUsageOID

out=Path(__file__).parent/'fixtures'/'tls'; out.mkdir(parents=True,exist_ok=True)
start=datetime(2020,1,1,tzinfo=timezone.utc); end=datetime(2040,1,1,tzinfo=timezone.utc)
def name(text):return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME,text)])
ca_key=rsa.generate_private_key(public_exponent=65537,key_size=2048)
ca=x509.CertificateBuilder().subject_name(name('Getman TEST ONLY CA')).issuer_name(name('Getman TEST ONLY CA')).public_key(ca_key.public_key()).serial_number(x509.random_serial_number()).not_valid_before(start).not_valid_after(end).add_extension(x509.BasicConstraints(ca=True,path_length=0),critical=True).add_extension(x509.KeyUsage(digital_signature=True,key_encipherment=False,key_cert_sign=True,crl_sign=True,content_commitment=False,data_encipherment=False,key_agreement=False,encipher_only=False,decipher_only=False),critical=True).sign(ca_key,hashes.SHA256())
(out/'ca.pem').write_bytes(ca.public_bytes(serialization.Encoding.PEM))
for role in ('server','client'):
    key=rsa.generate_private_key(public_exponent=65537,key_size=2048)
    builder=x509.CertificateBuilder().subject_name(name('localhost' if role=='server' else 'Getman TEST ONLY CLIENT')).issuer_name(ca.subject).public_key(key.public_key()).serial_number(x509.random_serial_number()).not_valid_before(start).not_valid_after(end).add_extension(x509.BasicConstraints(ca=False,path_length=None),critical=True).add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH if role=='server' else ExtendedKeyUsageOID.CLIENT_AUTH]),critical=False)
    if role=='server':builder=builder.add_extension(x509.SubjectAlternativeName([x509.DNSName('localhost')]),critical=False)
    cert=builder.sign(ca_key,hashes.SHA256())
    (out/f'{role}.pem').write_bytes(cert.public_bytes(serialization.Encoding.PEM))
    (out/f'{role}.key').write_bytes(key.private_bytes(serialization.Encoding.PEM,serialization.PrivateFormat.PKCS8,serialization.NoEncryption()))
print('Generated local test-only certificates:',out)
