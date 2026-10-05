<!DOCTYPE html>
<html>
  <head>
    <meta http-equiv="Content-Type" content="text/html; charset=utf-8">
    <style>
      /* Email styles need to be inline */
    </style>
  </head>

  <body>
    <p>Hey there,</p>

<p>{{ inviter_name | escape }} invited you to join <strong>{{ account_name | escape }}</strong> on {{ app_name | escape }} as {{ role_phrase }}.</p>

<p><a href="{{ url | escape }}">Accept the invitation</a></p>

<p>This invitation expires on {{ expires_on }}.</p>

<hr>

<p>Have questions or need help? Just reply to this email and our support team will help you sort it out.</p>

  </body>
</html>
