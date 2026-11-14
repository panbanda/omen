<?php

use function curl_setopt as configure;
use function openssl_encrypt as encrypt;
use function shell_exec as run;
use function sha1 as legacy_hash;
use function unserialize as revive;

function command_execution($command)
{
    system($command);
    run($command);
    escapeshellarg($command);
    $text = "system(\$command)";
    // system($command)
    return $text;
}

function code_evaluation($expression)
{
    eval($expression);
    intval($expression);
    $text = "eval(\$expression)";
    // eval($expression)
    return $text;
}

function unsafe_deserialization($blob)
{
    unserialize($blob);
    revive($blob);
    json_decode($blob);
    $text = "unserialize(\$blob)";
    // unserialize($blob)
    return $text;
}

function sql_string_construction($pdo, $name)
{
    $pdo->query("SELECT * FROM users WHERE name = '" . $name . "'");
    $pdo->query("SELECT 1");
    $text = "\$pdo->query(\$statement)";
    // $pdo->query($statement)
    return $text;
}

function weak_hash($data)
{
    md5($data);
    legacy_hash($data);
    hash("sha256", $data);
    $text = "md5(\$data)";
    // md5($data)
    return $text;
}

function weak_cipher($data, $key)
{
    openssl_encrypt($data, "des-ecb", $key);
    encrypt($data, "rc4", $key);
    openssl_encrypt($data, "aes-256-gcm", $key);
    $text = "openssl_encrypt(\$data, 'des-ecb')";
    // openssl_encrypt($data, "des-ecb", $key)
    return $text;
}

function tls_verification_disabled($handle)
{
    curl_setopt($handle, CURLOPT_SSL_VERIFYPEER, false);
    configure($handle, CURLOPT_SSL_VERIFYHOST, 0);
    curl_setopt($handle, CURLOPT_SSL_VERIFYPEER, true);
    $text = "curl_setopt(\$handle, CURLOPT_SSL_VERIFYPEER, false)";
    // curl_setopt($handle, CURLOPT_SSL_VERIFYPEER, false)
    return $text;
}

function hardcoded_credential()
{
    $token = "AKIAJ3KD92LQ7MZP5XWB";
    $name = "AKIAEXAMPLE";
    // AKIAJ3KD92LQ7MZP5XWB
    return [$token, $name];
}
