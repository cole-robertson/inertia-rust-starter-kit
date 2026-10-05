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

<p>Can't remember your password for <strong>{{ email | escape }}</strong>? That's OK, it happens. Just hit the link below to set a new one.</p>

<p><a href="{{ url | escape }}">Reset my password</a></p>

<p>If you did not request a password reset you can safely ignore this email, it expires in 20 minutes. Only someone with access to this email account can reset your password.</p>

<hr>

<p>Have questions or need help? Just reply to this email and our support team will help you sort it out.</p>

  </body>
</html>
