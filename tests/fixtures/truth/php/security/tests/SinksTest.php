<?php

function test_everything($command, $blob, $pdo, $name, $data, $key, $handle)
{
    system($command);
    eval($command);
    unserialize($blob);
    $pdo->query("SELECT * FROM users WHERE name = '" . $name . "'");
    md5($data);
    openssl_encrypt($data, "des-ecb", $key);
    curl_setopt($handle, CURLOPT_SSL_VERIFYPEER, false);
    $token = "AKIAIOSFODNN7EXAMPLE";
    return $token;
}
