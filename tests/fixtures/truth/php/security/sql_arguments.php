<?php

function sql_arguments($link, $name)
{
    mysql_query("SELECT 1", "a" . $name);
    mysql_query("SELECT " . $name, $link);
    mysqli_query($link, "SELECT '$name'");
    pg_query("SELECT " . $name);
}
