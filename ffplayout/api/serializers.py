from django.contrib.auth.models import User

from rest_framework import serializers

from api.models import GuiSettings


class UserSerializer(serializers.ModelSerializer):
    new_password = serializers.CharField(write_only=True, required=False)
    old_password = serializers.CharField(write_only=True, required=False)

    class Meta:
        model = User
        fields = ['id', 'username', 'old_password',
                  'new_password', 'email']

    def update(self, instance, validated_data):
        print(validated_data)
        instance.password = validated_data.get('password', instance.password)

        if 'new_password' in validated_data and \
                'old_password' in validated_data:
            if not validated_data['new_password']:
                raise serializers.ValidationError({'new_password': 'not found'})

            if not validated_data['old_password']:
                raise serializers.ValidationError({'old_password': 'not found'})

            if not instance.check_password(validated_data['old_password']):
                raise serializers.ValidationError(
                    {'old_password': 'wrong password'})

            if validated_data['new_password'] and \
                    instance.check_password(validated_data['old_password']):
                # instance.password = validated_data['new_password']
                instance.set_password(validated_data['new_password'])
                instance.save()
                return instance
        elif 'email' in validated_data:
            instance.email = validated_data['email']
            instance.save()
            return instance
        return instance


class GuiSettingsSerializer(serializers.ModelSerializer):

    class Meta:
        model = GuiSettings
        fields = '__all__'

    def get_fields(self, *args, **kwargs):
        fields = super().get_fields(*args, **kwargs)
        request = self.context.get('request')
        if request is not None and not request.parser_context.get('kwargs'):
            fields.pop('id', None)
        return fields
